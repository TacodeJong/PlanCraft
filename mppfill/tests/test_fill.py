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


if __name__ == "__main__":
    unittest.main()
