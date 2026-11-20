# Microsoft Planner export (developer note)

Sanitized integration notes for PlanCraft → Microsoft Planner. Derived from a tenant probe (2026-11-20). **No credentials, client secrets, refresh tokens, or private keys belong in this file or in git.**

Status: MVP implemented in `plancraft-app` (`planner_export` module, File → Export to Microsoft Planner). Auth uses device code + keyring. A **public** multi-tenant client id may ship in `DEFAULT_CLIENT_ID`; **secrets and tokens never** go in git.

---

## Product requirement: Plan 1+ and dependencies

MVP must export **plan / buckets / tasks / assignees / dates / % complete** plus **dependencies** (finish-to-start at minimum; SS/FF/SF when the target API supports them).

| License / plan surface | Dependencies |
| --- | --- |
| M365 / A1 basic Planner board | View only; **cannot create** deps |
| **Planner Plan 1+** (Premium / Project for the web) | **Required for MVP** — create FS (and typically SS/FF/SF) via the schedule API |
| Plan 3 lag/lead | Optional gap; not MVP-blocking |

**Implication:** a board-only push through Graph **v1.0** `plannerTask` is **not** enough for MVP. Real dependency rows need a **premium** plan (Dataverse project + Project Scheduling Service), not a basic Graph plan. Graph **beta** `plans_v2` documents dependency POST, but every permission on that page is listed as **Not supported** — treat beta as preview only until GA.

---

## Endpoints (public hosts)

| Purpose | Host / path |
| --- | --- |
| Graph API | `https://graph.microsoft.com` |
| Entra token (v2) | `https://login.microsoftonline.com/{tenant-id}/oauth2/v2.0/token` |
| Entra device code | `https://login.microsoftonline.com/{tenant-id}/oauth2/v2.0/devicecode` |
| Planner web | `https://planner.cloud.microsoft` |
| Dataverse discovery (EU) | `https://globaldisco.crm4.dynamics.com/api/discovery/v2.0/Instances` |
| Dataverse org (EU cluster) | `https://{org}.crm4.dynamics.com/api/data/v9.2/...` |

Unauthenticated `GET /v1.0/planner/plans` returns **401**. Personal Microsoft accounts are rejected; use a work/school account in the target tenant.

### First Graph reads after sign-in

```http
GET https://graph.microsoft.com/v1.0/me/planner/plans
GET https://graph.microsoft.com/v1.0/me/memberOf/microsoft.graph.group?$select=id,displayName
GET https://graph.microsoft.com/beta/me/planner/plans_v2
```

Id shape: basic plan ids are ~28-character case-sensitive strings; premium projects are GUIDs (`msdyn_projectid`). Graph `GET /planner/plans/{id}` returns basic plans only.

### Premium / schedule path (deps)

1. Discover Dataverse instance → org URL.
2. Token audience is the **org URL** (`https://{org}.crm4.dynamics.com/.default`), not Graph.
3. Read `$metadata` / attribute option sets **before** writing (link-type integers and column names differ between Project for the web and Project Operations).
4. Import inside a PSS **operation set** (create buckets → tasks → dependencies → assignments → execute), then read engine fields back.

Representative PSS actions (org-relative): `msdyn_PssCreateV1` / `msdyn_PssCreateV2`, `msdyn_ExecuteOperationSetV1`, resource contour updates. Cap ~200 operations per set.

---

## PlanCraft → Planner mapping

| PlanCraft | Basic Graph v1.0 board | Premium Plan 1+ (PSS / Dataverse) | Notes for MVP |
| --- | --- | --- | --- |
| Project / schedule title | `POST /planner/plans` (`owner` = M365 group id, `title`) | `msdyn_project` | Prefer **premium** when deps are required |
| Board columns / groups | `POST /planner/buckets` (`name`, `planId`, `orderHint`) | `msdyn_projectbucket` | Order hints: use Planner rules (`" !"` = first); do not copy returned hints blindly |
| Task | `POST /planner/tasks` (`title`, `planId`, `bucketId`, dates, `percentComplete`, `priority`, `assignments`, `appliedCategories`) | `msdyn_projecttask` (`msdyn_subject`, outline, effort, scheduled start/end, bucket bind, optional parent) | Client-supplied task GUIDs help later binds in one operation set |
| Notes | Patch task **details** (`description`) — separate etag from task | Task notes column when metadata allows | Task etag ≠ details etag; `412` → re-read and retry |
| Checklist | Details `checklist` | `msdyn_projectchecklist` | |
| Labels | `appliedCategories` + plan-details category names | `msdyn_projecttasktolabel` / label rename | |
| WBS / children | Checklist only (no parent task) | `msdyn_parenttask`, `msdyn_outlinelevel` | Premium only for real hierarchy |
| Milestone | No field (zero-duration still a normal task) | Prefer start=end + naming; direct `msdyn_ismilestone` writes are often rejected by the engine | Confirm flag after `ExecuteOperationSet`; UI toggle if still false |
| **Link / predecessor** | **None** | `msdyn_projecttaskdependency` (pred/succ binds + link-type option-set int) | **MVP path.** Delete+recreate to change. Needs real dates for the engine |
| Link type FS/SS/FF/SF | — | Map PlanCraft link type → environment option-set integer (`msdyn_projecttaskdependencylinktype` on Project for the web) | Do not hardcode Project Operations `msdyn_linktype` values unless metadata says that column exists |
| Lag / lead | — | Project-file `delay` / units; confirm Dataverse column via metadata | Optional; Plan 3 / out of MVP |
| Resource assignment | Entra user id on `assignments` | Team member + `msdyn_resourceassignment`; effort on task (`msdyn_effort`) | Resolve people via Graph `/users` before assign |
| Effort / work | No field | `msdyn_effort` (hours); contour is a separate PSS call | |

Keep a durable map: PlanCraft task key → Planner / `msdyn_projecttaskid` for updates and dependency recreate.

### Suggested import order (premium / MVP)

1. Delegated sign-in; classify basic vs premium plans.
2. Create or select premium project; ensure group membership and Plan 1+ license on the user.
3. Create team members.
4. Open one operation set; queue buckets → summary/leaf tasks → **dependencies** → assignments → checklist/labels as needed.
5. Execute; read back finish dates, milestone flag, successor dates.
6. Persist id map for later sync.

Basic board-only (non-MVP): group membership → plan → buckets → tasks/assignments → details patches.

---

## Sign in (end-user path)

PlanCraft uses the **device-code** flow with a **public multi-tenant client**. Secrets and tokens never go in git or the project file. A public client id **may** ship in `DEFAULT_CLIENT_ID` (`planner_export/defaults.rs`).

1. **File → Export to Microsoft Planner…**
2. **Sign in with Microsoft…** → open the device-login page → enter the code → finish with your work account.
3. For Plan 1+: set the Dataverse org URL → **Export**. (Basic board: pick an M365 group.)

Default tenant authority is `organizations`. Override tenant/client under **Advanced connection** only if needed.

If `DEFAULT_CLIENT_ID` is still empty, open Advanced once and paste the public client id from your admin (or set the constant / `PLANCRAFT_ENTRA_CLIENT_ID` at build time). You still do **not** register an Entra app from this dialog.

### One-time: create the public client (maintainer / admin)

Portal: [Entra → App registrations](https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade)

1. **New registration** — *Accounts in any organizational directory* (multi-tenant); **Redirect URI blank**.
2. **Authentication** → **Allow public client flows** = **Yes**.
3. **API permissions** (all **Delegated**): Graph `Tasks.ReadWrite`, `Group.ReadWrite.All`, `User.Read`; Dataverse `user_impersonation` for Plan 1+.
4. **Grant admin consent** in the home tenant.
5. Put the **Application (client) ID** into `DEFAULT_CLIENT_ID` (safe to commit — it is public) or Advanced. Helper: `scripts/register-entra-public-client.sh` after `az login`.

### Runtime notes

1. Caller should be a work/school member, hold **Planner Plan 1+** for dependencies, and be a member of the target M365 group.
2. Graph uses the Graph token; premium PSS uses a **separate** token for the Dataverse org audience (`{orgUrl}/.default`).
3. Never log raw tokens.

---
## Public vs secret; where values live

| Value | Classification | Storage |
| --- | --- | --- |
| Default tenant (`organizations`) / overrides | Public | Source defaults and/or local settings |
| Application (client) id | Public | Source `DEFAULT_CLIENT_ID` and/or local settings override |
| Dataverse org URL | Non-secret | Local settings |
| Redirect URI, scopes, Graph/Dataverse **host patterns** | Public | Docs / source OK (no org-specific ids) |
| Client **secret** (confidential client only) | **Secret** | OS keyring — **never** source, docs, or BACKLOG |
| Refresh token / access token | **Secret** | Keyring |
| User password / private keys | **Secret** | Never stored; interactive auth only |

### Keyring pattern (match Distill Plan)

- Service name under the PlanCraft keyring family (`planner.refresh_token`, optional `planner.client_secret`).
- Tokens/secrets never in `settings.json`, project files, or git.
- Scrub secrets from logs and error messages before display.

---

## Explicit non-goals for this note

- No committed credentials or probe dumps.
- No implementation under `plancraft/` from this document alone.
- Graph beta `plans_v2` dependency import is **not** the production path until permissions are supported.
- Lag/lead, full Merlin fidelity (stretch/span, calendars, leveling), and Planner-JSON round-trip remain out of MVP.

## References

- [Planner overview (Graph v1.0)](https://learn.microsoft.com/en-us/graph/api/resources/planner-overview?view=graph-rest-1.0)
- [Create plannerTask](https://learn.microsoft.com/en-us/graph/api/planner-post-tasks?view=graph-rest-1.0)
- [Project schedule APIs](https://learn.microsoft.com/en-us/dynamics365/project-operations/project-management/schedule-api-preview)
- [Schedule APIs with Power Automate](https://learn.microsoft.com/en-us/dynamics365/project-operations/project-management/scheduling-apis-powerautomate)
- [Project for the web export content definition](https://learn.microsoft.com/en-us/project-for-the-web/export-project-content-definition)
- [Plan conversion](https://learn.microsoft.com/en-us/planner/plan-conversion)
- [Basic vs premium plans](https://support.microsoft.com/en-us/planner/compare-microsoft-planner-basic-vs-premium-plans)
