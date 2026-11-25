"""MSPDI parsing and mapping, without writing an .mpp."""

from __future__ import annotations

import unittest
from datetime import date
from pathlib import Path
from tempfile import TemporaryDirectory

from pymppwriter import validate

from mppfill.errors import MppFillError
from mppfill.export import check_output
from mppfill.map import to_project
from mppfill.mspdi import load_bytes

NS = "http://schemas.microsoft.com/project"

PLAN = f"""<?xml version="1.0" encoding="UTF-8"?>
<Project xmlns="{NS}">
  <Name>Cycle</Name>
  <Title>Cycle</Title>
  <StartDate>2026-11-02T08:00:00</StartDate>
  <CalendarUID>1</CalendarUID>
  <MinutesPerDay>480</MinutesPerDay>
  <CurrencySymbol>€</CurrencySymbol>
  <CurrencyCode>EUR</CurrencyCode>
  <Calendars>
    <Calendar>
      <UID>1</UID>
      <Name>Standaard</Name>
      <IsBaseCalendar>1</IsBaseCalendar>
      <WeekDays>
        <WeekDay><DayType>1</DayType><DayWorking>0</DayWorking></WeekDay>
        <WeekDay><DayType>2</DayType><DayWorking>1</DayWorking>
          <WorkingTimes>
            <WorkingTime><FromTime>08:00:00</FromTime><ToTime>12:00:00</ToTime></WorkingTime>
            <WorkingTime><FromTime>13:00:00</FromTime><ToTime>17:00:00</ToTime></WorkingTime>
          </WorkingTimes>
        </WeekDay>
        <WeekDay><DayType>7</DayType><DayWorking>0</DayWorking></WeekDay>
      </WeekDays>
      <Exceptions>
        <Exception>
          <Name>Kerst</Name>
          <Type>2</Type>
          <Month>11</Month>
          <MonthDay>25</MonthDay>
          <TimePeriod>
            <FromDate>2026-01-01T00:00:00</FromDate>
            <ToDate>2027-12-31T23:59:00</ToDate>
          </TimePeriod>
          <DayWorking>0</DayWorking>
        </Exception>
      </Exceptions>
    </Calendar>
  </Calendars>
  <Tasks>
    <Task><UID>0</UID><ID>0</ID><Name>Cycle</Name><OutlineLevel>0</OutlineLevel></Task>
    <Task>
      <UID>1</UID><ID>1</ID><Name>Phase</Name><OutlineLevel>1</OutlineLevel><Summary>1</Summary>
      <Start>2026-11-02T08:00:00</Start><Finish>2026-11-04T08:00:00</Finish>
      <Duration>PT16H0M0S</Duration>
    </Task>
    <Task>
      <UID>2</UID><ID>2</ID><Name>Work</Name><OutlineLevel>2</OutlineLevel>
      <Start>2026-11-02T08:00:00</Start><Finish>2026-11-03T17:00:00</Finish>
      <Duration>PT16H0M0S</Duration><DurationFormat>7</DurationFormat>
      <PercentComplete>40</PercentComplete>
      <Notes>hello</Notes>
      <Priority>700</Priority>
      <ConstraintType>4</ConstraintType>
      <ConstraintDate>2026-11-02T08:00:00</ConstraintDate>
      <ExtendedAttribute><FieldID>188743731</FieldID><Value>team</Value></ExtendedAttribute>
    </Task>
    <Task>
      <UID>3</UID><ID>3</ID><Name>Gate</Name><OutlineLevel>2</OutlineLevel>
      <Start>2026-11-04T08:00:00</Start><Finish>2026-11-04T08:00:00</Finish>
      <Duration>PT0H0M0S</Duration><Milestone>1</Milestone>
      <PredecessorLink>
        <PredecessorUID>2</PredecessorUID><Type>1</Type><LinkLag>-4800</LinkLag><LagFormat>7</LagFormat>
      </PredecessorLink>
    </Task>
    <Task>
      <UID>4</UID><ID>4</ID><Name>After</Name><OutlineLevel>1</OutlineLevel>
      <Start>2026-11-05T08:00:00</Start><Finish>2026-11-05T17:00:00</Finish><Duration>PT8H0M0S</Duration>
      <PredecessorLink><PredecessorUID>2</PredecessorUID><Type>0</Type><LinkLag>4800</LinkLag><LagFormat>7</LagFormat></PredecessorLink>
      <PredecessorLink><PredecessorUID>2</PredecessorUID><Type>3</Type><LinkLag>0</LinkLag><LagFormat>7</LagFormat></PredecessorLink>
      <PredecessorLink><PredecessorUID>2</PredecessorUID><Type>2</Type><LinkLag>-9600</LinkLag><LagFormat>7</LagFormat></PredecessorLink>
    </Task>
  </Tasks>
  <Resources>
    <Resource><UID>1</UID><Name>Ada</Name><Type>1</Type><MaxUnits>1</MaxUnits><Initials>A</Initials></Resource>
    <Resource><UID>2</UID><Name>Paper</Name><Type>0</Type></Resource>
  </Resources>
  <Assignments>
    <Assignment><TaskUID>2</TaskUID><ResourceUID>1</ResourceUID><Units>0.5</Units></Assignment>
    <Assignment><TaskUID>2</TaskUID><ResourceUID>2</ResourceUID><Units>1</Units></Assignment>
  </Assignments>
</Project>
"""


class ParseTests(unittest.TestCase):
    def test_plan_maps_hierarchy_links_and_fields(self):
        plan = load_bytes(PLAN.encode("utf-8"))
        self.assertEqual(plan.title, "Cycle")
        self.assertEqual(plan.minutes_per_day, 480)
        self.assertEqual([task.uid for task in plan.tasks], [1, 2, 3, 4])
        project, notes = to_project(plan)
        validate(project)
        by_uid = {task.uid: task for task in project.tasks}
        self.assertEqual(by_uid[1].name, "Phase")
        self.assertEqual(by_uid[2].parent_uid, 1)
        self.assertEqual(by_uid[2].outline_level, 2)
        self.assertEqual(by_uid[2].duration_days, 2.0)
        self.assertEqual(by_uid[2].percent_complete, 40)
        self.assertEqual(by_uid[2].constraint, "SNET")
        self.assertEqual(by_uid[2].text[1], "team")
        self.assertEqual(by_uid[2].notes, "hello")
        self.assertEqual(by_uid[3].duration_days, 0.0)
        kinds = {(link.pred_uid, link.succ_uid, link.type, link.lag_days) for link in project.relations}
        self.assertIn((2, 3, "FS", -1.0), kinds)
        self.assertIn((2, 4, "FF", 1.0), kinds)
        self.assertIn((2, 4, "SS", 0.0), kinds)
        self.assertIn((2, 4, "SF", -2.0), kinds)
        self.assertEqual([(item.uid, item.name, item.max_units) for item in project.resources], [(1, "Ada", 1.0)])
        self.assertEqual(project.assignments[0].units, 0.5)
        self.assertTrue(any("material" in note for note in notes))
        self.assertEqual(project.currency_symbol, "€")
        self.assertEqual(project.default_calendar, "Standaard")
        standard = project.calendar
        self.assertIsNotNone(standard)
        assert standard is not None
        self.assertIsNone(standard.week[6])  # Sunday
        self.assertIsNone(standard.week[5])  # Saturday
        self.assertEqual(standard.week[0], [(8 * 60, 12 * 60), (13 * 60, 17 * 60)])
        kerst = [item for item in standard.exceptions if item.name == "Kerst"]
        self.assertEqual([item.start for item in kerst], [date(2026, 12, 25), date(2027, 12, 25)])
        self.assertTrue(all(item.finish == item.start for item in kerst))

    def test_percent_lag_is_reported_and_dropped(self):
        xml = f"""<Project xmlns="{NS}"><MinutesPerDay>480</MinutesPerDay><StartDate>2026-11-02T08:00:00</StartDate>
          <Tasks>
            <Task><UID>1</UID><ID>1</ID><Name>A</Name><OutlineLevel>1</OutlineLevel>
              <Start>2026-11-02T08:00:00</Start><Finish>2026-11-02T17:00:00</Finish><Duration>PT8H0M0S</Duration></Task>
            <Task><UID>2</UID><ID>2</ID><Name>B</Name><OutlineLevel>1</OutlineLevel>
              <Start>2026-11-03T08:00:00</Start><Finish>2026-11-03T17:00:00</Finish><Duration>PT8H0M0S</Duration>
              <PredecessorLink><PredecessorUID>1</PredecessorUID><Type>1</Type><LinkLag>500</LinkLag><LagFormat>19</LagFormat></PredecessorLink>
            </Task>
          </Tasks></Project>"""
        project, notes = to_project(load_bytes(xml.encode("utf-8")))
        self.assertEqual(project.relations[0].lag_days, 0.0)
        self.assertTrue(any("percent lag" in note for note in notes))

    def test_task_ids_are_renumbered_when_they_go_backwards(self):
        xml = f"""<Project xmlns="{NS}"><StartDate>2026-11-02T08:00:00</StartDate>
          <Tasks>
            <Task><UID>30</UID><ID>1</ID><Name>Phase</Name><OutlineLevel>1</OutlineLevel>
              <Start>2026-11-02T08:00:00</Start><Finish>2026-11-03T17:00:00</Finish><Duration>PT16H0M0S</Duration></Task>
            <Task><UID>10</UID><ID>2</ID><Name>Work</Name><OutlineLevel>2</OutlineLevel>
              <Start>2026-11-02T08:00:00</Start><Finish>2026-11-02T17:00:00</Finish><Duration>PT8H0M0S</Duration></Task>
            <Task><UID>40</UID><ID>3</ID><Name>Next</Name><OutlineLevel>1</OutlineLevel>
              <Start>2026-11-03T08:00:00</Start><Finish>2026-11-03T17:00:00</Finish><Duration>PT8H0M0S</Duration>
              <PredecessorLink><PredecessorUID>10</PredecessorUID><Type>1</Type><LinkLag>0</LinkLag></PredecessorLink></Task>
          </Tasks>
          <Resources><Resource><UID>7</UID><Name>Ada</Name><Type>1</Type></Resource></Resources>
          <Assignments><Assignment><TaskUID>10</TaskUID><ResourceUID>7</ResourceUID><Units>1</Units></Assignment></Assignments>
        </Project>"""
        project, _notes = to_project(load_bytes(xml.encode("utf-8")))
        validate(project)
        self.assertEqual([(task.uid, task.name, task.parent_uid) for task in project.tasks], [
            (1, "Phase", 0),
            (2, "Work", 1),
            (3, "Next", 0),
        ])
        self.assertEqual([(link.pred_uid, link.succ_uid) for link in project.relations], [(2, 3)])
        self.assertEqual([(item.task_uid, item.resource_uid) for item in project.assignments], [(2, 7)])

    def test_outline_gap_is_an_error(self):
        xml = f"""<Project xmlns="{NS}"><StartDate>2026-11-02T08:00:00</StartDate><Tasks>
          <Task><UID>1</UID><ID>1</ID><Name>A</Name><OutlineLevel>1</OutlineLevel>
            <Start>2026-11-02T08:00:00</Start><Finish>2026-11-02T17:00:00</Finish><Duration>PT8H0M0S</Duration></Task>
          <Task><UID>2</UID><ID>2</ID><Name>B</Name><OutlineLevel>3</OutlineLevel>
            <Start>2026-11-02T08:00:00</Start><Finish>2026-11-02T17:00:00</Finish><Duration>PT8H0M0S</Duration></Task>
        </Tasks></Project>"""
        with self.assertRaises(MppFillError):
            to_project(load_bytes(xml.encode("utf-8")))

    def test_cycle_is_rejected_by_the_library_validator(self):
        xml = f"""<Project xmlns="{NS}"><StartDate>2026-11-02T08:00:00</StartDate><Tasks>
          <Task><UID>1</UID><ID>1</ID><Name>A</Name><OutlineLevel>1</OutlineLevel>
            <Start>2026-11-02T08:00:00</Start><Finish>2026-11-02T17:00:00</Finish><Duration>PT8H0M0S</Duration>
            <PredecessorLink><PredecessorUID>2</PredecessorUID><Type>1</Type><LinkLag>0</LinkLag></PredecessorLink></Task>
          <Task><UID>2</UID><ID>2</ID><Name>B</Name><OutlineLevel>1</OutlineLevel>
            <Start>2026-11-03T08:00:00</Start><Finish>2026-11-03T17:00:00</Finish><Duration>PT8H0M0S</Duration>
            <PredecessorLink><PredecessorUID>1</PredecessorUID><Type>1</Type><LinkLag>0</LinkLag></PredecessorLink></Task>
        </Tasks></Project>"""
        project, _notes = to_project(load_bytes(xml.encode("utf-8")))
        with self.assertRaises(ValueError):
            validate(project)

    def test_finish_before_start_is_an_error(self):
        xml = f"""<Project xmlns="{NS}"><StartDate>2026-11-02T08:00:00</StartDate><Tasks>
          <Task><UID>1</UID><ID>1</ID><Name>A</Name><OutlineLevel>1</OutlineLevel>
            <Start>2026-11-03T08:00:00</Start><Finish>2026-11-02T17:00:00</Finish><Duration>PT8H0M0S</Duration></Task>
        </Tasks></Project>"""
        with self.assertRaises(MppFillError):
            to_project(load_bytes(xml.encode("utf-8")))

    def test_malformed_xml_and_doctype_are_refused(self):
        with self.assertRaises(MppFillError):
            load_bytes(b"<Project><Tasks></Project>")
        with self.assertRaises(MppFillError):
            load_bytes(b"<?xml version='1.0'?><!DOCTYPE Project [<!ENTITY x 'y'>]><Project/>")
        with self.assertRaises(MppFillError):
            load_bytes(b"<NotAProject/>")

    def test_output_cannot_replace_the_template_or_an_existing_file(self):
        with TemporaryDirectory() as folder:
            root = Path(folder)
            template = root / "template.mpp"
            template.write_bytes(b"unused")
            source = root / "plan.xml"
            source.write_bytes(b"<Project/>")
            with self.assertRaises(MppFillError):
                check_output(template, template, source, force=True)
            with self.assertRaises(MppFillError):
                check_output(source, template, source, force=True)
            existing = root / "out.mpp"
            existing.write_bytes(b"x")
            with self.assertRaises(MppFillError):
                check_output(existing, template, source, force=False)
            check_output(existing, template, source, force=True)
            with self.assertRaises(MppFillError):
                check_output(root / "out.xml", template, source, force=True)


if __name__ == "__main__":
    unittest.main()
