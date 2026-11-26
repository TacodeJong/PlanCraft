"""Planner import request building. No network."""

from __future__ import annotations

import unittest

from datetime import datetime

from pymppwriter import Task

from mppfill.errors import MppFillError
from mppfill.map import apply_planner_profile
from mppfill.planner import (
    check_lines,
    failure_message,
    parse_import,
    progress_entity,
    project_url,
    milestone_updates,
    schedule_ready,
    staging_body,
    tenant_from_header,
)


class PlannerRequestTest(unittest.TestCase):
    def test_staging_row_binds_the_lookups(self) -> None:
        body = staging_body(
            "PLANNER-cyclus",
            "d97b51d0-c54e-ed11-bba2-000d3abb10e5",
            "e15448ef-21c3-f111-aaaf-00224886f516",
        )
        self.assertEqual(body["msdyn_projectname"], "PLANNER-cyclus")
        self.assertEqual(body["msdyn_projectschedulemode"], 192350001)
        self.assertTrue(body["msdyn_importtonewproject"])
        self.assertIn("msdyn_projectWorkHourTemplate@odata.bind", body)
        self.assertNotIn("_msdyn_projectworkhourtemplate_value", body)
        self.assertTrue(body["msdyn_Projectmanager@odata.bind"].endswith("e15448ef-21c3-f111-aaaf-00224886f516)"))

    def test_import_result_reads_the_project_and_the_warnings(self) -> None:
        project, status, warnings = parse_import(
            {
                "ProjectId": "e87728c4-67b3-43ea-8d10-7b4c7b67cd42",
                "Result": '{"importStatus":201,"importMppWarnings":{"W_IMPORT_RESOURCE_NOTSUPPORTED":79}}',
            }
        )
        self.assertEqual(project, "e87728c4-67b3-43ea-8d10-7b4c7b67cd42")
        self.assertEqual(status, 201)
        self.assertEqual(warnings["W_IMPORT_RESOURCE_NOTSUPPORTED"], 79)

    def test_sign_in_header_names_the_tenant(self) -> None:
        header = (
            "Bearer authorization_uri=https://login.microsoftonline.com/"
            "c2f7fe95-d06e-4ec1-b007-f790209a8f2e/oauth2/authorize, "
            "resource_id=https://orgc1fde974.crm4.dynamics.com/"
        )
        self.assertEqual(tenant_from_header(header), "c2f7fe95-d06e-4ec1-b007-f790209a8f2e")
        self.assertEqual(tenant_from_header(""), "")

    def test_the_planner_profile_pins_leaf_starts_and_drops_the_rest(self) -> None:
        start = datetime(2027, 1, 4, 8, 0)
        finish = datetime(2027, 1, 13, 17, 0)
        summary = Task(uid=1, name="Controles", start=start, finish=finish, outline_level=1)
        leaf = Task(
            uid=2,
            name="Afsluiting",
            start=start,
            finish=finish,
            outline_level=2,
            parent_uid=1,
            constraint="FNET",
            constraint_date=finish,
            deadline=finish,
            manual=True,
        )
        milestone = Task(
            uid=3,
            name="Gate",
            start=start,
            finish=finish,
            duration_days=0,
            outline_level=2,
            parent_uid=1,
            constraint="MSO",
            constraint_date=start,
        )
        point = Task(
            uid=4,
            name="Point",
            start=start,
            finish=start,
            duration_days=0,
            outline_level=2,
            parent_uid=1,
        )
        notes = apply_planner_profile([summary, leaf, milestone, point])
        self.assertIsNone(summary.constraint)
        self.assertEqual(summary.duration_days, 1.0)
        self.assertEqual(leaf.constraint, "SNET")
        self.assertEqual(leaf.constraint_date, start)
        self.assertIsNone(leaf.deadline)
        self.assertFalse(leaf.manual)
        self.assertEqual(milestone.duration_days, 0.125)
        self.assertEqual(milestone.finish, datetime(2027, 1, 4, 9, 0))
        self.assertEqual(point.duration_days, 0.125)
        self.assertEqual(point.finish, datetime(2027, 1, 4, 9, 0))
        self.assertTrue(any("start-no-earlier-than" in note for note in notes))
        self.assertTrue(any("deadlines" in note for note in notes))

    def test_the_date_check_prints_the_documented_tasks(self) -> None:
        lines = check_lines(
            [
                {
                    "msdyn_subject": "Afsluiting van de administratie",
                    "msdyn_scheduledstart": "2027-01-04T09:00:00Z",
                    "msdyn_scheduledend": "2027-01-13T17:00:00Z",
                },
                {"msdyn_subject": "Something else", "msdyn_scheduledstart": "2026-10-15T08:00:00Z"},
            ]
        )
        self.assertEqual(lines, ["Afsluiting van de administratie: 2027-01-04T09:00:00Z – 2027-01-13T17:00:00Z"])

    def test_the_project_link_opens_the_power_app_record(self) -> None:
        link = project_url(
            "https://orgc1fde974.crm4.dynamics.com",
            "d5cbeb07-c54e-ed11-bba1-000d3abda444",
            "e4804c3a-1e3f-41ca-8a25-62c007f713e1",
        )
        self.assertIn("pagetype=entityrecord", link)
        self.assertIn("etn=msdyn_project", link)
        self.assertIn("id=e4804c3a-1e3f-41ca-8a25-62c007f713e1", link)
        self.assertIn("extraqs=TabToFocus%3DPexWebTab", link)

    def test_an_unfinished_milestone_is_not_left_at_100_percent(self) -> None:
        marks = [("Gate", 0), ("Done gate", 100), ("Gate", 0)]
        rows = [
            {
                "msdyn_projecttaskid": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
                "msdyn_subject": "Work item",
                "msdyn_ismilestone": False,
                "msdyn_duration": 2,
                "msdyn_progress": 0,
            },
            {
                "msdyn_projecttaskid": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
                "msdyn_subject": "Gate",
                "msdyn_ismilestone": True,
                "msdyn_progress": 1,
            },
            {
                "msdyn_projecttaskid": "cccccccc-cccc-cccc-cccc-cccccccccccc",
                "msdyn_subject": "Done gate",
                "msdyn_ismilestone": True,
                "msdyn_progress": 1,
            },
            {
                "msdyn_projecttaskid": "dddddddd-dddd-dddd-dddd-dddddddddddd",
                "msdyn_subject": "  gate ",
                "msdyn_ismilestone": True,
                "msdyn_progress": 0,
            },
        ]
        self.assertEqual(
            milestone_updates(marks, rows),
            [("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", 0.0)],
        )
        rows.insert(
            1,
            {
                "msdyn_projecttaskid": "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee",
                "msdyn_subject": "Gate",
                "msdyn_summary": True,
                "msdyn_duration": 0,
                "msdyn_progress": 1,
            },
        )
        self.assertEqual(
            milestone_updates(marks, rows),
            [("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", 0.0)],
        )
        self.assertEqual(progress_entity("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", 0.0)["msdyn_progress"], 0.0)
        self.assertEqual(progress_entity("cccccccc-cccc-cccc-cccc-cccccccccccc", 1.0)["msdyn_progress"], 1.0)

    def test_the_schedule_is_not_ready_until_the_finish_is_filled(self) -> None:
        self.assertFalse(schedule_ready(""))
        self.assertTrue(schedule_ready("2027-12-15T17:00:00Z"))

    def test_a_failed_import_reports_planners_message(self) -> None:
        text = failure_message({"Result": '{"code":"-1","message":"Failed to import a project.","innerError":null}'})
        self.assertEqual(text, "Failed to import a project.")

    def test_a_broken_import_result_is_refused(self) -> None:
        with self.assertRaises(MppFillError):
            parse_import({"ProjectId": "x", "Result": "not-json"})


if __name__ == "__main__":
    unittest.main()
