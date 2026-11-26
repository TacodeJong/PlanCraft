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
from pymppwriter.writer import (
    ASSN_META_SIZE,
    ASSN_NATIVE,
    NATIVE,
    PRJ,
    PROJECT_CLSID,
    RSC_NATIVE,
    TASK_META_SIZE,
    MppWriter,
    advance_working,
)


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
    Marking the date present keeps the start. A milestone has no length, and the
    importer reads that as 100% complete. A zero-length milestone is given one
    hour from its start. An unfinished one keeps that hour remaining, so its
    percent stays 0. A finished one stays at 100%.
    The template's actual-work records
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
        try:
            header2, _, metas2 = writer._meta(f"{task}/Fixed2Meta", 92)
        except KeyError:
            header2, metas2 = b"", []
        records = blocks.split_fixed_data(writer._get(f"{task}/FixedData"), metas)
        kind_field = writer.task_fm.get(NATIVE["CONSTRAINT_TYPE"])
        date_field = writer.task_fm.get(NATIVE["CONSTRAINT_DATE"])
        date_bit = writer.task_bit.get(NATIVE["CONSTRAINT_DATE"])
        meta_out: list[bytes] = []
        meta2_out: list[bytes] = []
        edited: list[bytes] = []
        opened = False
        for index, (record, meta) in enumerate(zip(records, metas)):
            row = bytearray(record)
            bits = bytearray(meta)
            bits2 = bytearray(metas2[index]) if index < len(metas2) else bytearray()
            if len(row) > 100:
                _mark_constraint_date(row, bits, kind_field, date_field, date_bit)
                _mark_percent_present(writer, bits, bits2)
                opened = _milestone_day(writer, row, bits, bits2) or opened
            edited.append(bytes(row))
            meta_out.append(bytes(bits))
            if bits2:
                meta2_out.append(bytes(bits2))
        if opened:
            writer._set(f"{task}/FixedData", b"".join(edited))
        writer._set(f"{task}/FixedMeta", blocks.build_fixed_meta(header, meta_out, len(b"".join(edited))))
        if header2 and meta2_out and len(meta2_out) == len(metas2):
            fixed2 = writer._get(f"{task}/Fixed2Data")
            writer._set(f"{task}/Fixed2Meta", blocks.build_fixed_meta(header2, meta2_out, len(fixed2)))
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


def milestone_percents(path: Path) -> list[tuple[str, int]]:
    """Name and percent complete of each milestone, in file order.

    Percent 0 is unfinished. Percent 100 is the only other value a milestone carries.
    """
    writer = MppWriter(str(path))
    try:
        percent = writer.task_fm.get(NATIVE["PERCENT_COMPLETE"])
        duration = writer.task_fm.get(NATIVE["DURATION"])
        uid_field = writer.task_fm.get(NATIVE["UNIQUE_ID"])
        milestone = writer.task_bit.get(NATIVE["MILESTONE"])
        if percent is None or duration is None or uid_field is None or milestone is None:
            return []
        if percent.block != 0 or duration.block != 0 or uid_field.block != 0:
            return []
        task = f"{PRJ}/TBkndTask"
        _, _, metas = writer._meta(f"{task}/FixedMeta", TASK_META_SIZE)
        try:
            _, _, metas2 = writer._meta(f"{task}/Fixed2Meta", 92)
        except KeyError:
            metas2 = []
        records = blocks.split_fixed_data(writer._get(f"{task}/FixedData"), metas)
        _, table, _ = blocks.parse_var_meta(writer._get(f"{task}/VarMeta"))
        data = writer._get(f"{task}/Var2Data")
        found: list[tuple[str, int]] = []
        for index, record in enumerate(records):
            if len(record) <= max(percent.offset + 2, duration.offset + 4, uid_field.offset + 4):
                continue
            meta2 = metas2[index] if index < len(metas2) else b""
            if not blocks.meta_bit(metas[index], meta2, milestone):
                continue
            uid = struct.unpack_from("<I", record, uid_field.offset)[0]
            slot = table.get(uid, {}).get(NATIVE["NAME"])
            if slot is None:
                continue
            name = blocks.read_var(data, slot).decode("utf-16-le", "replace").rstrip("\x00")
            value = struct.unpack_from("<H", record, percent.offset)[0]
            found.append((name, 100 if value >= 100 else 0))
        return found
    finally:
        del writer
        gc.collect()


def _milestone_day(writer: MppWriter, row: bytearray, meta: bytearray, meta2: bytearray) -> bool:
    """Give a zero-length milestone one hour, so its percent is read like a task.

    A row with no length is stored as 100% complete. One hour with that hour
    still remaining stays at 0%. One hour with nothing remaining stays at 100%.
    The start stays. The finish is one working hour later.
    """
    summary = writer.task_bit.get(NATIVE["SUMMARY"])
    if summary is not None and blocks.meta_bit(meta, meta2, summary):
        return False
    duration = writer.task_fm.get(NATIVE["DURATION"])
    remaining = writer.task_fm.get(NATIVE["REMAINING_DURATION"])
    percent = writer.task_fm.get(NATIVE["PERCENT_COMPLETE"])
    if duration is None or remaining is None or percent is None:
        return False
    if duration.block != 0 or remaining.block != 0 or percent.block != 0:
        return False
    if duration.offset + 4 > len(row) or remaining.offset + 4 > len(row) or percent.offset + 2 > len(row):
        return False
    if struct.unpack_from("<i", row, duration.offset)[0] != 0:
        return False
    done = struct.unpack_from("<H", row, percent.offset)[0] >= 100
    struct.pack_into("<i", row, duration.offset, 600)
    struct.pack_into("<i", row, remaining.offset, 0 if done else 600)
    actual = writer.task_fm.get(NATIVE["ACTUAL_DURATION"])
    if actual is not None and actual.block == 0 and actual.offset + 4 <= len(row):
        struct.pack_into("<i", row, actual.offset, 600 if done else 0)
    start = _read_stamp(writer, row, "START")
    if start is not None:
        finish = advance_working(start, 600)
        for name in ("FINISH", "EARLY_FINISH", "LATE_FINISH"):
            _write_stamp(writer, row, name, finish)
    for name in ("PERCENT_COMPLETE", "PERCENT_WORK_COMPLETE"):
        bit = writer.task_bit.get(NATIVE[name])
        if bit is not None:
            blocks.set_meta_bit(meta, meta2, bit, True)
    milestone = writer.task_bit.get(NATIVE["MILESTONE"])
    if milestone is not None:
        blocks.set_meta_bit(meta, meta2, milestone, False)
    return True


def _read_stamp(writer: MppWriter, row: bytearray, name: str):
    field = writer.task_fm.get(NATIVE[name])
    if field is None or field.block != 0 or field.offset + 4 > len(row):
        return None
    return blocks.decode_timestamp(row, field.offset)


def _write_stamp(writer: MppWriter, row: bytearray, name: str, moment) -> None:
    field = writer.task_fm.get(NATIVE[name])
    if field is None or field.block != 0:
        return
    raw = blocks.encode_timestamp(moment)
    if field.offset + len(raw) > len(row):
        return
    row[field.offset:field.offset + len(raw)] = raw


def _mark_percent_present(writer: MppWriter, meta: bytearray, meta2: bytearray) -> None:
    """Keep the stored percent, including zero.

    pymppwriter only marks percent complete present when it is above zero. A
    milestone then has duration 0 and no percent, which Planner reads as finished.
    """
    bit = writer.task_bit.get(NATIVE["PERCENT_COMPLETE"])
    if bit is None:
        return
    blocks.set_meta_bit(meta, meta2, bit, True)


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
