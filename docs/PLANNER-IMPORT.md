# Import a plan into Planner Premium (MPP)

How to get a PlanCraft plan into Planner Premium (Project for the web) through the Power App, and how to tell that the import actually ran.

This failed the same way twice (2026-11-24): the staging form said **Opgeslagen** and **Importeren** did nothing. The file was stored. The import API was never called.

No tokens belong in this file.

## Working steps (verified 2026-11-25)

These are the steps that put the real start and finish dates into Planner. The import call is the same one described further down. The file has to be the one those steps produce.

In the app: **Bestand → Verstuur naar Planner Premium…**, or the **Verstuur** button on the toolbar. PlanCraft writes the Planner XML and runs `mppfill`. It finds `mppfill` beside the program and keeps the template in its own folder after the first send (from Downloads or `D:\Drenthe`). The first time, a browser window opens so you can sign in. Later sends reuse that sign-in. When the dates are in, the browser opens the Tasks tab. Leave that tab open until the tasks are on screen. Each send creates a new plan.

1. Export Microsoft Project XML with the Planner profile, from the plan file:

   ```text
   plancraft-cli export-xml --planner plan.plancraft plan.xml
   ```

   In the app: **Bestand → Exporteer voor Planner Premium…**

   That XML is the only source to turn into an `.mpp`. It keeps each leaf’s calculated start and finish, writes the start again as “start no earlier than”, and leaves out manual scheduling, deadlines, and finish constraints (must-start-on, must-finish-on, as-late-as-possible, start-no-later-than, finish-no-earlier-than, finish-no-later-than). Milestones have duration 0. Work-resource names are written on the task, in the text column aliased **Resources** (Text7 on this plan).

   An XML saved back out of Microsoft Project still has those other constraints. `push` rewrites it to this profile before filling the template. `D:\Drenthe\PLANNER-cyclus1.xml` is such a file. An `.mpp` built from it without that rewrite is refused: Planner answers `Failed to import a project.` (`code` `-1`) and can still leave an empty project with no tasks. That empty project is not the plan.

2. Fill the original template and write the `.mpp`. The template is `D:\Drenthe\PLANNER-cyclus.mpp`, an MPP saved by desktop Project and then used only as the pymppwriter prototype. It is not the plan, and a Project-saved copy of the finished plan is not a substitute for it. On 2026-11-25 that template was not on disk. Do not point `--template` at `Downloads\PLANNER-cyclus.mpp`; filling that file produces an `.mpp` Planner refuses.

   From the `mppfill` folder, once the template is back in place:

   ```text
   .\.venv\Scripts\python.exe -m mppfill push plan.xml --out plan.mpp --force --name PLANNER-cyclus
   ```

   `push` first applies the Planner profile to the XML: each leaf start is pinned with start-no-earlier-than, summaries stay unconstrained, and manual scheduling and deadlines are dropped. It then fills the template, applies the corrections the importer needs, and uploads:

   - The constraint date is written, and its presence bit is set. Without that bit Planner ignores the date and schedules every task from the project start (2026-10-15).
   - The estimated-duration mark stays set. Clearing it makes Planner ignore the constraint and snap the task back to the project start.
   - Actual-work records copied from the template are removed on every task at 0% complete. Left in place, Planner treats them as progress and gives the task no length, so the finish equals the start. Tasks that already have progress keep those records.

3. Sign in when the browser window opens, on this PC, with the work account. The organisation blocks the code-on-a-website sign-in, so the command does not use that. The sign-in is stored in `%LOCALAPPDATA%\mppfill` and is not printed. The next run reuses it.

4. The command creates a new staging row, sets the project name, schedule mode **Vaste duur** (`192350001`), work-hour template **Standaardwerksjabloon** (`d97b51d0-c54e-ed11-bba2-000d3abb10e5`), and the signed-in user as project manager, uploads the `.mpp`, and calls `msdyn_ImportProjectV2`. Lookups use the navigation properties `msdyn_projectWorkHourTemplate` and `msdyn_Projectmanager`. Each run is a new row. Uploading a second file onto an existing row returns `0x80048105`.

5. Success is `importStatus` **201**. The only expected warning is `W_IMPORT_RESOURCE_NOTSUPPORTED` (79 on this plan). The command prints the project id and a link, waits until the dates are filled, then opens that link in the browser. The link is the project record plus `extraqs=TabToFocus=PexWebTab`. The project form reads `TabToFocus` and opens the Tasks tab (label **Taken**). Loading that tab is what puts the plan on the Planner Premium list; the Dynamics row alone does not. Check dates, not the duration column:

   | Task | Start | Finish |
   | --- | --- | --- |
   | Controles en correcties administratie | 2027-01-04 09:00 | 2027-01-15 17:00 |
   | Afsluiting van de administratie | 2027-01-04 09:00 | 2027-01-13 17:00 |
   | Controle op de boekingen. | 2027-01-04 09:00 | 2027-01-13 17:00 |
   | De eerste verwerking van memoboekingen… | 2027-01-14 09:00 | 2027-01-14 17:00 |
   | De laatste memoboekingen… | 2027-01-14 09:00 | 2027-01-15 17:00 |

   The project runs 2026-10-15 08:00 through 2027-12-15 17:00. Task times show 09:00 and 17:00 because that is the work-hour template. A one-day task ending on the same calendar day is right. A multi-day task must end on a later day. Add the column **Resources** to see the names. Assignments are not created.

The file that passed this check is `D:\Drenthe\PLANNER-cyclus7.mpp` (525824 bytes). Import that file again with:

```text
.\.venv\Scripts\python.exe -m mppfill push --mpp "D:\Drenthe\PLANNER-cyclus7.mpp" --name PLANNER-cyclus
```

That import was repeated on 2026-11-25 and returned `importStatus` 201. Project `2e7344c2-ad5b-4581-8143-b9b1a4436d0c`. The earlier import of the same file, the one checked against the table above, is project `e87728c4-67b3-43ea-8d10-7b4c7b67cd42`.

## What you produce first

Follow **Working steps** above. The short form is: Planner-profile XML, the original template `D:\Drenthe\PLANNER-cyclus.mpp`, then `mppfill push`. A Project-saved `.mpp` of a plan that already has the right dates can be imported with `mppfill push --mpp plan.mpp`. It is not a template for a new fill.

## Where the button is

Planner Power App → **Plannen** → **Mijn actieve plannen** → **Meer opdrachten** → **Importeer vanuit Excel of Mpp** → **Project Desktop** → **Importeren vanuit MPP**.

That opens a new row of `msdyn_projectimportstaging` (“Fasering van projectimport”) and asks for **MPP-bestand** (`msdyn_importedmppfile`).

Org used here: `https://orgc1fde974.crm4.dynamics.com`. App id `d5cbeb07-c54e-ed11-bba1-000d3abda444`.

## Why Importeren does not import

**Importeren** only saves the form. The import runs in `OnPostSave` (`executeProjectImportAPI` → bound action `msdyn_ImportProjectV2`).

Attaching the file auto-saves the form. The header then says **Opgeslagen**. The form is no longer dirty, so the next click on **Importeren** does not save and does not call the API. There is often no error. UCI 2200000006 (“Cannot save due to no dirty changes”) is the same failure when it does surface.

Check the staging row. If these are empty, the import has not run:

| Field | Logical name |
| --- | --- |
| Project name | `msdyn_projectname` |
| Schedule mode | `msdyn_projectschedulemode` |
| Work-hour template | `_msdyn_projectworkhourtemplate_value` |
| Project manager | `_msdyn_projectmanager_value` |
| Created project | `_msdyn_targetproject_value` |

`msdyn_importtonewproject` is already true on a new row. That is not enough.

Do not upload a second file onto the same staging row. Replacing the file returns `0x80048105` (concurrent upload). Start a new staging row instead.

## What actually starts the import

The plugin refuses the call until the name, the calendar, and the manager are set. Bind lookups with the navigation property. The logical name returns `0x80048d19`.

```http
PATCH /api/data/v9.2/msdyn_projectimportstagings({staging-id})
Content-Type: application/json
If-Match: *

{
  "msdyn_projectname": "PLANNER-cyclus",
  "msdyn_projectschedulemode": 192350001,
  "msdyn_projectWorkHourTemplate@odata.bind": "/msdyn_workhourtemplates(d97b51d0-c54e-ed11-bba2-000d3abb10e5)",
  "msdyn_Projectmanager@odata.bind": "/systemusers({systemuser-id})"
}
```

`192350001` is **Vaste duur** (fixed duration). The template above is **Standaardwerksjabloon**, the calendar already used by plans in this org. `{systemuser-id}` is the importer’s `systemuser` row.

Then call the bound action. It takes no parameters:

```http
POST /api/data/v9.2/msdyn_projectimportstagings({staging-id})/Microsoft.Dynamics.CRM.msdyn_ImportProjectV2
Content-Type: application/json

{}
```

`importStatus` **201** is success. The body also returns `ProjectId` and `importMppWarnings`. Any other status is a failure. Task rows stay at 0 for a few seconds while the schedule service syncs; read `msdyn_projecttask` again after about 8 seconds.

## What the 2026-11-24 XML import did

Staging `8967bb04-fde7-f111-84e7-6045bd8a7273`, file `PLANNER-cyclus.mpp` (saved from the Planner-profile XML). Project `4c0662d2-1339-49f7-b198-931c586c1200`.

| | First import (original MPP) | This import (Planner XML → MPP) |
| --- | --- | --- |
| Manual | 14 | 0 |
| Deadline | 58 | 0 |
| Constraint type | 29 | 0 |
| Resource | 79 | 79 |
| Tasks | 165 | 165 |
| Assignments | not created | 0 |

The only remaining warning is `W_IMPORT_RESOURCE_NOTSUPPORTED` for all 79 work resources, so no assignments were created. “Opstellen concept jaarrekening” came in as 8 days (not the old 75-day stretch), but its start was the plan start (2026-10-15), not 18-01-2027.

## Resources become a task column

Planner Premium does not import resource rows. That is a published limit of “Move your project from Project desktop to Project for the web” (the list “Features that don't migrate” includes Resources). Changing the resource record does not make those 79 rows assignable.

The Planner XML profile therefore writes the assigned work-resource names on each task, in a text column aliased **Resources**. On this plan that column is **Text7**, because Text1–Text6 are already custom fields (including Planner Given Work). Text1–Text30 hold 255 characters; a longer list is cut and the export note says so.

After the next import, open the plan and use **Add column** → **Resources**. The names are there to assign from. The importer still will not create the assignment rows. Live assignments need the schedule API, which this user cannot call until an administrator grants create on `msdyn_operationset`.
