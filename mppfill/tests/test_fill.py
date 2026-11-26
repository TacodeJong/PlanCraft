"""Write an .mpp from XML using the configured template, when that file is present."""

from __future__ import annotations

import hashlib
import gc
import unittest
from datetime import datetime
from pathlib import Path
from tempfile import TemporaryDirectory

from pymppwriter import read_project

from mppfill.cli import main
from mppfill.export import DEFAULT_TEMPLATE

XML = """<?xml version="1.0" encoding="UTF-8"?>
<Project xmlns="http://schemas.microsoft.com/project">
  <Title>Probe</Title>
  <StartDate>2026-11-02T08:00:00</StartDate>
  <MinutesPerDay>480</MinutesPerDay>
  <Tasks>
    <Task>
      <UID>1</UID><ID>1</ID><Name>Phase</Name><OutlineLevel>1</OutlineLevel><Summary>1</Summary>
      <Start>2026-11-02T08:00:00</Start><Finish>2026-11-04T17:00:00</Finish>
      <Duration>PT24H0M0S</Duration>
    </Task>
    <Task>
      <UID>2</UID><ID>2</ID><Name>Work item</Name><OutlineLevel>2</OutlineLevel>
      <Start>2026-11-02T08:00:00</Start><Finish>2026-11-03T17:00:00</Finish>
      <Duration>PT16H0M0S</Duration><PercentComplete>25</PercentComplete><Notes>hello</Notes>
      <ConstraintType>4</ConstraintType><ConstraintDate>2026-11-02T08:00:00</ConstraintDate>
    </Task>
    <Task>
      <UID>3</UID><ID>3</ID><Name>Gate</Name><OutlineLevel>2</OutlineLevel><Milestone>1</Milestone>
      <Start>2026-11-04T08:00:00</Start><Finish>2026-11-04T08:00:00</Finish><Duration>PT0H0M0S</Duration>
      <PredecessorLink><PredecessorUID>2</PredecessorUID><Type>1</Type><LinkLag>0</LinkLag><LagFormat>7</LagFormat></PredecessorLink>
    </Task>
  </Tasks>
  <Resources>
    <Resource><UID>7</UID><Name>Ada</Name><Type>1</Type><MaxUnits>1</MaxUnits></Resource>
  </Resources>
  <Assignments>
    <Assignment><TaskUID>2</TaskUID><ResourceUID>7</ResourceUID><Units>1</Units></Assignment>
  </Assignments>
</Project>
"""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


@unittest.skipUnless(DEFAULT_TEMPLATE.is_file(), f"template not found at {DEFAULT_TEMPLATE}")
class TemplateTests(unittest.TestCase):
    def test_xml_round_trip_keeps_the_template_file(self):
        before = _sha256(DEFAULT_TEMPLATE)
        with TemporaryDirectory() as folder:
            xml = Path(folder) / "plan.xml"
            out = Path(folder) / "plan.mpp"
            xml.write_text(XML, encoding="utf-8")
            status = main(["xml-to-mpp", str(xml), "--template", str(DEFAULT_TEMPLATE), "--out", str(out)])
            self.assertEqual(status, 0)
            self.assertEqual(_sha256(DEFAULT_TEMPLATE), before)
            project = read_project(str(out))
            self.assertEqual(project.title, "Probe")
            by_uid = {task.uid: task for task in project.tasks}
            self.assertEqual(by_uid[1].name, "Phase")
            self.assertEqual(by_uid[2].name, "Work item")
            self.assertEqual(by_uid[2].notes, "hello")
            self.assertEqual(by_uid[2].percent_complete, 25)
            self.assertEqual(by_uid[2].constraint, "SNET")
            self.assertEqual(by_uid[2].parent_uid, 1)
            self.assertEqual(by_uid[3].name, "Gate")
            self.assertEqual(by_uid[3].duration_days, 0.0)
            self.assertEqual([(link.pred_uid, link.succ_uid, link.type) for link in project.relations], [(2, 3, "FS")])
            self.assertEqual([(item.uid, item.name) for item in project.resources], [(7, "Ada")])
            self.assertEqual([(item.task_uid, item.resource_uid) for item in project.assignments], [(2, 7)])
            self.assertEqual(project.start, datetime(2026, 11, 2, 8, 0))
            del project
            gc.collect()

    def test_existing_output_is_kept_without_force(self):
        with TemporaryDirectory() as folder:
            xml = Path(folder) / "plan.xml"
            out = Path(folder) / "plan.mpp"
            xml.write_text(XML, encoding="utf-8")
            out.write_bytes(b"keep")
            status = main(["xml-to-mpp", str(xml), "--template", str(DEFAULT_TEMPLATE), "--out", str(out)])
            self.assertEqual(status, 1)
            self.assertEqual(out.read_bytes(), b"keep")


def _template() -> Path | None:
    downloads = Path.home() / "Downloads" / "PLANNER-cyclus.mpp"
    for path in (DEFAULT_TEMPLATE, downloads):
        if path.is_file():
            return path
    return None


_TEMPLATE = _template()


@unittest.skipUnless(_TEMPLATE is not None, "template not found")
class MilestonePercentTests(unittest.TestCase):
    def test_unfinished_milestone_keeps_a_present_zero_percent(self):
        xml = XML.replace(
            "<Name>Gate</Name>",
            "<Name>Gate</Name><PercentComplete>0</PercentComplete>",
        )
        xml = xml.replace(
            "</Tasks>",
            """
    <Task>
      <UID>4</UID><ID>4</ID><Name>Done gate</Name><OutlineLevel>2</OutlineLevel><Milestone>1</Milestone>
      <Start>2026-11-04T08:00:00</Start><Finish>2026-11-04T08:00:00</Finish>
      <Duration>PT0H0M0S</Duration><PercentComplete>100</PercentComplete>
    </Task>
  </Tasks>""",
        )
        with TemporaryDirectory() as folder:
            src = Path(folder) / "plan.xml"
            out = Path(folder) / "plan.mpp"
            src.write_text(xml, encoding="utf-8")
            template = _TEMPLATE
            assert template is not None
            status = main(["xml-to-mpp", str(src), "--template", str(template), "--out", str(out)])
            self.assertEqual(status, 0)
            percents = _percents(out)
            self.assertEqual(percents["Gate"], (0, 1, 600, 600))
            self.assertEqual(percents["Done gate"], (100, 1, 600, 0))
            gc.collect()


def _percents(path: Path) -> dict[str, tuple[int, int, int, int]]:
    import struct

    from pymppwriter import blocks
    from pymppwriter.writer import MppWriter, NATIVE, PRJ, TASK_META_SIZE

    writer = MppWriter(str(path))
    try:
        percent = writer.task_fm.get(NATIVE["PERCENT_COMPLETE"])
        duration = writer.task_fm.get(NATIVE["DURATION"])
        remaining = writer.task_fm.get(NATIVE["REMAINING_DURATION"])
        uid_field = writer.task_fm.get(NATIVE["UNIQUE_ID"])
        bit = writer.task_bit.get(NATIVE["PERCENT_COMPLETE"])
        task = f"{PRJ}/TBkndTask"
        _, _, metas = writer._meta(f"{task}/FixedMeta", TASK_META_SIZE)
        try:
            _, _, metas2 = writer._meta(f"{task}/Fixed2Meta", 92)
        except KeyError:
            metas2 = []
        records = blocks.split_fixed_data(writer._get(f"{task}/FixedData"), metas)
        _, table, _ = blocks.parse_var_meta(writer._get(f"{task}/VarMeta"))
        data = writer._get(f"{task}/Var2Data")
        found: dict[str, tuple[int, int, int, int]] = {}
        if percent is None or duration is None or remaining is None or uid_field is None or bit is None:
            return found
        for index, record in enumerate(records):
            if len(record) <= max(percent.offset + 2, duration.offset + 4, remaining.offset + 4, uid_field.offset + 4):
                continue
            uid = struct.unpack_from("<I", record, uid_field.offset)[0]
            slot = table.get(uid, {}).get(NATIVE["NAME"])
            if slot is None:
                continue
            name = blocks.read_var(data, slot).decode("utf-16-le", "replace").rstrip("\x00")
            value = struct.unpack_from("<H", record, percent.offset)[0]
            span = struct.unpack_from("<i", record, duration.offset)[0]
            left = struct.unpack_from("<i", record, remaining.offset)[0]
            meta2 = metas2[index] if index < len(metas2) else b""
            present = blocks.meta_bit(metas[index], meta2, bit) or 0
            found[name] = (value, present, span, left)
        return found
    finally:
        del writer
        gc.collect()


if __name__ == "__main__":
    unittest.main()
