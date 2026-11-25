"""Make a populated MPP usable as a pymppwriter 0.4.3 template, and finish a written file so Planner keeps its dates.

pymppwriter clones variable data from a prototype task. It reads that task's
unique id from byte 0 of the fixed record. In PLANNER-cyclus.mpp byte 0 is the
row id and the unique id sits at the field-map offset, so the leaf prototype
has no name field and every written task comes out unnamed.

Keep only a name entry on each prototype and clear progress and constraint
fields copied from a real task. Otherwise every written task inherits that
task's percent complete, and Planner rejects the file. Resource rows from the
template are always dropped except the unnamed resource, so the XML plan
supplies the resources.
"""

from __future__ import annotations

import gc
import struct
from pathlib import Path

from pymppwriter import blocks as blocks
from pymppwriter.cfb import write_cfb
from pymppwriter.writer import ASSN_META_SIZE, ASSN_NATIVE, NATIVE, PRJ, PROJECT_CLSID, RSC_NATIVE, TASK_META_SIZE, MppWriter


def prepare_template(writer: MppWriter) -> None:
    _drop_template_resources(writer)
    name_id = NATIVE["NAME"]
    proto = writer.proto.get("task")
    uid_field = writer.task_fm.get(NATIVE["UNIQUE_ID"])
    if proto is None or uid_field is None:
        return
    existing = next((data for typ, data in proto["var"] if typ == name_id), None)
    payload = existing if existing is not None else _leaf_name(writer, uid_field.offset, name_id)
    _only_name(proto, payload)
    _blank_schedule_fields(writer, proto)
    _put_name_bit(writer, proto)
    summary = writer.proto.get("summary")
    if summary is not None:
        existing = next((data for typ, data in summary["var"] if typ == name_id), None)
        _only_name(summary, existing if existing is not None else payload)
        _blank_schedule_fields(writer, summary)
        _put_name_bit(writer, summary)


def _leaf_name(writer: MppWriter, offset: int, name_id: int) -> bytes:
    path = f"{PRJ}/TBkndTask"
    _, _, metas = writer._meta(f"{path}/FixedMeta", TASK_META_SIZE)
    records = blocks.split_fixed_data(writer._get(f"{path}/FixedData"), metas)
    _, table, _ = blocks.parse_var_meta(writer._get(f"{path}/VarMeta"))
    data = writer._get(f"{path}/Var2Data")
    for record in records:
        if len(record) <= 100 or offset + 4 > len(record):
            continue
        uid = struct.unpack_from("<I", record, offset)[0]
        if uid == 0:
            continue
        slot = table.get(uid, {}).get(name_id)
        if slot is not None:
            return blocks.read_var(data, slot)
    return blocks.encode_unicode("")


def _only_name(proto: dict, payload: bytes) -> None:
    proto["var"] = [(NATIVE["NAME"], payload)]
    meta = bytearray(proto["meta"])
    meta[2] = 1
    proto["meta"] = bytes(meta)


def _drop_template_resources(writer: MppWriter) -> None:
    """A blank template only has the unnamed resource. A real plan also has its
    people, and pymppwriter copies every resource row it found. Drop those so
    the XML file's resources are the ones in the output. The unnamed row
    (unique id 0) and the short stub rows stay."""
    field = writer.rsc_fm.get(RSC_NATIVE["UNIQUE_ID"])
    if field is None or field.block != 0:
        return
    kept = []
    for row in writer.rsc_rows:
        record = row["rec"]
        if len(record) <= 16 or field.offset + 4 > len(record):
            kept.append(row)
            continue
        uid = struct.unpack_from("<I", record, field.offset)[0]
        if uid == 0:
            kept.append(row)
    writer.rsc_rows = kept


def _put_name_bit(writer: MppWriter, proto: dict) -> None:
    meta = bytearray(proto["meta"])
    meta2 = bytearray(proto["meta2"])
    writer._put_bit(meta, meta2, "NAME", True)
    proto["meta"] = bytes(meta)
    proto["meta2"] = bytes(meta2)


def publish_schedule(path: Path) -> None:
    """Make Planner's importer keep the task's start and finish.

    pymppwriter writes the constraint date but leaves its presence bit clear, so
    Planner ignores the date and schedules every task from the project start.
    Marking the date present keeps the start. The template's actual-work records
    are also copied onto every assignment. Planner treats that as progress and
    either gives an unstarted task no length or refuses the file. Drop those
    records where nothing is complete. Project Viewer reads the date bytes
    either way.
    """
    blob = _scheduled_bytes(path)
    temporary = path.with_name(path.name + ".sched")
    temporary.write_bytes(blob)
    gc.collect()
    temporary.replace(path)


def _scheduled_bytes(path: Path) -> bytes:
    writer = MppWriter(str(path))
    try:
        task = f"{PRJ}/TBkndTask"
        header, _, metas = writer._meta(f"{task}/FixedMeta", TASK_META_SIZE)
        records = blocks.split_fixed_data(writer._get(f"{task}/FixedData"), metas)
        kind_field = writer.task_fm.get(NATIVE["CONSTRAINT_TYPE"])
        date_field = writer.task_fm.get(NATIVE["CONSTRAINT_DATE"])
        date_bit = writer.task_bit.get(NATIVE["CONSTRAINT_DATE"])
        meta_out: list[bytes] = []
        for record, meta in zip(records, metas):
            bits = bytearray(meta)
            if len(record) > 100:
                _mark_constraint_date(record, bits, kind_field, date_field, date_bit)
            meta_out.append(bytes(bits))
        writer._set(f"{task}/FixedMeta", blocks.build_fixed_meta(header, meta_out, len(b"".join(records))))
        _drop_cloned_actuals(writer)
        return write_cfb(writer.root, root_clsid=PROJECT_CLSID)
    finally:
        del writer
        gc.collect()


def _drop_cloned_actuals(writer: MppWriter) -> None:
    """Remove copied actual-work records from assignments whose task has not started."""
    idle = _idle_tasks(writer)
    if not idle:
        return
    assn = f"{PRJ}/TBkndAssn"
    header, _, metas = writer._meta(f"{assn}/FixedMeta", ASSN_META_SIZE)
    try:
        header2, _, metas2 = writer._meta(f"{assn}/Fixed2Meta", 53)
    except KeyError:
        header2, metas2 = b"", []
    records = blocks.split_fixed_data(writer._get(f"{assn}/FixedData"), metas)
    task_field = writer.assn_fm.get(ASSN_NATIVE["TASK_UNIQUE_ID"])
    uid_field = writer.assn_fm.get(ASSN_NATIVE["UNIQUE_ID"])
    if task_field is None or uid_field is None or task_field.block != 0 or uid_field.block != 0:
        return
    drop: set[int] = set()
    for record in records:
        if len(record) <= max(task_field.offset + 4, uid_field.offset + 4):
            continue
        if struct.unpack_from("<I", record, task_field.offset)[0] in idle:
            drop.add(struct.unpack_from("<I", record, uid_field.offset)[0])
    if not drop:
        return
    var_header, _, entries = blocks.parse_var_meta(writer._get(f"{assn}/VarMeta"))
    data = writer._get(f"{assn}/Var2Data")
    actual_types = {ASSN_NATIVE["ACTUAL_WORK_DATA"], ASSN_NATIVE["ACTUAL_IRREGULAR_DATA"]}
    kept: list[tuple[int, int, bytes, int]] = []
    removed: dict[int, int] = {}
    for uid, offset, typ, high in entries:
        if typ in actual_types and uid in drop:
            removed[uid] = removed.get(uid, 0) + 1
            continue
        kept.append((uid, typ, blocks.read_var(data, offset), high))
    if not removed:
        return
    meta_out, data_out = _var_blocks(var_header, kept)
    writer._set(f"{assn}/VarMeta", meta_out)
    writer._set(f"{assn}/Var2Data", data_out)
    size_key = blocks.PROPS_VAR2DATA_SIZE.get("TBkndAssn")
    if size_key is not None and size_key in writer.props:
        writer.props[size_key] = struct.pack("<I", len(data_out))
        writer._set(f"{PRJ}/Props", blocks.build_props(writer.props_hdr, writer.props, writer.props_order))
    clear_bits = [bit for typ in actual_types if (bit := writer.assn_bit.get(typ)) is not None]
    rebuilt: list[bytes] = []
    rebuilt2: list[bytes] = []
    for index, (record, meta) in enumerate(zip(records, metas)):
        bits = bytearray(meta)
        bits2 = bytearray(metas2[index]) if index < len(metas2) else bytearray()
        if len(record) > uid_field.offset + 4:
            uid = struct.unpack_from("<I", record, uid_field.offset)[0]
            count = removed.get(uid, 0)
            if count and bits[2] > 0:
                bits[2] = max(0, bits[2] - count)
            if count and bits2:
                for bit in clear_bits:
                    blocks.set_meta_bit(bits, bits2, bit, False)
        rebuilt.append(bytes(bits))
        if bits2:
            rebuilt2.append(bytes(bits2))
    writer._set(f"{assn}/FixedMeta", blocks.build_fixed_meta(header, rebuilt, len(b"".join(records))))
    if header2 and rebuilt2:
        fixed2 = writer._get(f"{assn}/Fixed2Data")
        writer._set(f"{assn}/Fixed2Meta", blocks.build_fixed_meta(header2, rebuilt2, len(fixed2)))


def _idle_tasks(writer: MppWriter) -> set[int]:
    percent = writer.task_fm.get(NATIVE["PERCENT_COMPLETE"])
    uid_field = writer.task_fm.get(NATIVE["UNIQUE_ID"])
    if percent is None or uid_field is None or percent.block != 0 or uid_field.block != 0:
        return set()
    task = f"{PRJ}/TBkndTask"
    _, _, metas = writer._meta(f"{task}/FixedMeta", TASK_META_SIZE)
    records = blocks.split_fixed_data(writer._get(f"{task}/FixedData"), metas)
    idle: set[int] = set()
    for record in records:
        if len(record) <= max(percent.offset + 2, uid_field.offset + 4):
            continue
        if struct.unpack_from("<H", record, percent.offset)[0] == 0:
            idle.add(struct.unpack_from("<I", record, uid_field.offset)[0])
    return idle


def _var_blocks(header: bytes, values: list[tuple[int, int, bytes, int]]) -> tuple[bytes, bytes]:
    meta = bytearray(header)
    body = bytearray()
    entries = bytearray()
    for uid, typ, payload, high in sorted(values, key=lambda item: (item[0], item[1])):
        entries += struct.pack("<IIHH", uid, len(body), typ, high)
        body += struct.pack("<I", len(payload)) + payload
    struct.pack_into("<I", meta, 8, len(values))
    struct.pack_into("<I", meta, 20, len(body))
    return bytes(meta) + bytes(entries), bytes(body)


def _mark_constraint_date(row: bytes | bytearray, meta: bytearray, kind_field, date_field, date_bit: int | None) -> None:
    if date_bit is None or kind_field is None or date_field is None:
        return
    if kind_field.block != 0 or date_field.block != 0:
        return
    if kind_field.offset + 2 > len(row) or date_field.offset + 4 > len(row):
        return
    kind = struct.unpack_from("<H", row, kind_field.offset)[0]
    raw = bytes(row[date_field.offset:date_field.offset + 4])
    if kind == 0 or raw == b"\xff\xff\xff\xff":
        return
    blocks.set_meta_bit(meta, bytearray(1), date_bit, True)


def _blank_schedule_fields(writer: MppWriter, proto: dict) -> None:
    record = bytearray(proto["rec"])
    meta = bytearray(proto["meta"])
    meta2 = bytearray(proto["meta2"])
    for name, fmt, value in (
        ("PERCENT_COMPLETE", "<H", 0),
        ("PERCENT_WORK_COMPLETE", "<H", 0),
        ("ACTUAL_DURATION", "<i", 0),
        ("ACTUAL_WORK", "<d", 0.0),
        ("CONSTRAINT_TYPE", "<H", 0),
    ):
        writer._put(record, name, fmt, value)
        writer._put_bit(meta, meta2, name, False)
    for name in ("ACTUAL_START", "ACTUAL_FINISH", "STOP", "RESUME", "CONSTRAINT_DATE", "DEADLINE"):
        writer._put_ts(record, name, None)
        writer._put_bit(meta, meta2, name, False)
    proto["rec"] = bytes(record)
    proto["meta"] = bytes(meta)
    proto["meta2"] = bytes(meta2)
