"""Import an .mpp into Planner Premium through the Dataverse Web API.

Sign-in uses the public client published for Dataverse samples. The token stays
in the process and in the per-user MSAL cache. It is never printed.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import webbrowser
from pathlib import Path

from .errors import MppFillError
from .limits import MAX_FILE_BYTES
from .template_fix import publish_schedule

# Org, app and calendar used by docs/PLANNER-IMPORT.md. Flags and environment
# variables override them.
DEFAULT_ORG = "https://orgc1fde974.crm4.dynamics.com"
DEFAULT_APP = "d5cbeb07-c54e-ed11-bba1-000d3abda444"
DEFAULT_CALENDAR = "d97b51d0-c54e-ed11-bba2-000d3abb10e5"
# Vaste duur. Planner keeps the duration from the file instead of recalculating it from work.
FIXED_DURATION = 192350001
# Dynamics 365 Example Client Application, Microsoft's public Dataverse sample client. No secret.
_CLIENT_ID = "51f81489-12ee-4a9e-aaae-a2591f45987d"
_GUID = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
# Tasks tab on the Information form (label "Taken"). The form opens it when the
# address carries TabToFocus. Loading that tab lists the plan in Planner Premium.
_TASKS_TAB = "PexWebTab"


def push_project(
    mpp: Path,
    org: str,
    calendar_id: str,
    name: str,
    app_id: str = DEFAULT_APP,
) -> str:
    """Upload `mpp` into a new Planner project. Returns the new project id."""
    org = _org(org)
    calendar_id = _guid(calendar_id, "calendar")
    app_id = _guid(app_id, "app")
    data = _prepared(mpp)
    token = _token(org)
    manager = _whoami(org, token)
    staging = _create_staging(org, token, name, calendar_id, manager)
    _upload(org, token, staging, mpp.name, data)
    project_id, warnings = _import(org, token, staging)
    link = project_url(org, app_id, project_id)
    print(f"Project {project_id}")
    print(link)
    for code, count in sorted(warnings.items()):
        print(f"- {code}: {count}")
    _wait_for_schedule(org, token, project_id)
    _open_project(link)
    return project_id


def project_url(org: str, app_id: str, project_id: str) -> str:
    """Power App page for the project, opened on the Tasks tab.

    The project id from the import is the record id. The form's own loader reads
    TabToFocus and selects that tab. Loading the Tasks tab is what lists the plan
    in Planner Premium.
    """
    focus = urllib.parse.quote(f"TabToFocus={_TASKS_TAB}")
    return (
        f"{org}/main.aspx?appid={app_id}&pagetype=entityrecord&etn=msdyn_project&id={project_id}&extraqs={focus}"
    )


def _open_project(url: str) -> None:
    try:
        opened = webbrowser.open(url)
    except OSError:
        opened = False
    if opened:
        print("Opened the Tasks tab so Planner Premium can list the plan.")
        return
    print("Open the link above on the Tasks tab so Planner Premium can list the plan.")


def staging_body(name: str, calendar_id: str, manager_id: str) -> dict:
    """JSON body for a new import row. Lookups use the navigation property."""
    return {
        "msdyn_projectname": name,
        "msdyn_projectschedulemode": FIXED_DURATION,
        "msdyn_importtonewproject": True,
        "msdyn_projectWorkHourTemplate@odata.bind": f"/msdyn_workhourtemplates({calendar_id})",
        "msdyn_Projectmanager@odata.bind": f"/systemusers({manager_id})",
    }


def parse_import(payload: dict) -> tuple[str, int, dict]:
    """Return project id, importStatus and the warning counts from ImportProjectV2."""
    project_id = str(payload.get("ProjectId") or "")
    raw = payload.get("Result") or "{}"
    try:
        result = json.loads(raw) if isinstance(raw, str) else raw
    except json.JSONDecodeError as exc:
        raise MppFillError("Planner returned an import result that is not JSON") from exc
    if not isinstance(result, dict):
        raise MppFillError("Planner returned an import result that is not a record")
    status = result.get("importStatus")
    warnings = result.get("importMppWarnings") or {}
    if not isinstance(warnings, dict):
        warnings = {}
    return project_id, status if isinstance(status, int) else -1, {str(k): v for k, v in warnings.items()}


def _prepared(path: Path) -> bytes:
    """Copy the file, apply the Planner date fix, and return the bytes to upload."""
    if not path.is_file():
        raise MppFillError(f"{path} is not a file")
    if path.stat().st_size > MAX_FILE_BYTES:
        raise MppFillError(f"{path} is larger than {MAX_FILE_BYTES} bytes")
    try:
        with tempfile.TemporaryDirectory() as folder:
            copy = Path(folder) / path.name
            shutil.copyfile(path, copy)
            publish_schedule(copy)
            return copy.read_bytes()
    except OSError as exc:
        raise MppFillError(f"cannot prepare {path}: {exc.strerror or exc}") from exc


def _org(value: str) -> str:
    org = value.strip().rstrip("/")
    if not org.startswith("https://") or not org.endswith(".dynamics.com"):
        raise MppFillError("the Planner address must be an https Dynamics URL")
    return org


def _guid(value: str, label: str) -> str:
    text = value.strip().strip("{}")
    if not _GUID.match(text):
        raise MppFillError(f"the {label} id is not a GUID")
    return text


def _token(org: str) -> str:
    try:
        import msal
    except ImportError as exc:
        raise MppFillError("sign-in needs the msal package; install it with pip install msal") from exc
    cache = msal.SerializableTokenCache()
    store = _cache_path()
    if store.is_file():
        try:
            cache.deserialize(store.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            cache = msal.SerializableTokenCache()
    app = msal.PublicClientApplication(
        _CLIENT_ID,
        authority=f"https://login.microsoftonline.com/{_tenant(org)}",
        token_cache=cache,
    )
    scopes = [f"{org}/user_impersonation"]
    result = None
    accounts = app.get_accounts()
    if accounts:
        result = app.acquire_token_silent(scopes, account=accounts[0])
    if not result:
        # Device-code sign-in is refused by this organisation's access policy.
        print("A browser window will open so you can sign in.")
        result = app.acquire_token_interactive(scopes)
    if cache.has_state_changed:
        _write_cache(store, cache.serialize())
    token = result.get("access_token") if isinstance(result, dict) else None
    if not token:
        raise MppFillError(_auth_error(result if isinstance(result, dict) else {}))
    return token


def tenant_from_header(header: str) -> str:
    """Read the Azure tenant id from a Dataverse WWW-Authenticate header."""
    found = re.search(r"login\.microsoftonline\.com/([0-9a-fA-F-]{36})", header or "")
    if found is None or not _GUID.match(found.group(1)):
        return ""
    return found.group(1)


def _tenant(org: str) -> str:
    override = os.environ.get("MPPFILL_TENANT", "").strip()
    if override:
        return _guid(override, "tenant")
    request = urllib.request.Request(f"{org}/api/data/v9.2/", method="GET")
    header = ""
    try:
        urllib.request.urlopen(request, timeout=30)
    except urllib.error.HTTPError as exc:
        header = exc.headers.get("WWW-Authenticate") or ""
    except urllib.error.URLError as exc:
        raise MppFillError(f"cannot reach Planner ({exc.reason})") from exc
    tenant = tenant_from_header(header)
    if not tenant:
        raise MppFillError("Planner did not name its sign-in directory")
    return tenant


def _cache_path() -> Path:
    root = os.environ.get("LOCALAPPDATA") or str(Path.home())
    return Path(root) / "mppfill" / "msal-cache.bin"


def _write_cache(path: Path, text: str) -> None:
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    except OSError as exc:
        raise MppFillError(f"cannot store the sign-in cache: {exc.strerror or exc}") from exc


def _auth_error(result: dict) -> str:
    detail = result.get("error_description") or result.get("error") or "sign-in failed"
    text = str(detail).split("\n", 1)[0]
    return text[:300]


def _whoami(org: str, token: str) -> str:
    status, body = _call("GET", f"{org}/api/data/v9.2/WhoAmI", token)
    payload = _json(status, body)
    user = payload.get("UserId")
    if not isinstance(user, str) or not _GUID.match(user):
        raise MppFillError("Planner did not return the signed-in user")
    return user


def _create_staging(org: str, token: str, name: str, calendar_id: str, manager_id: str) -> str:
    status, body = _call(
        "POST",
        f"{org}/api/data/v9.2/msdyn_projectimportstagings",
        token,
        json.dumps(staging_body(name, calendar_id, manager_id)).encode("utf-8"),
        {"Content-Type": "application/json", "Prefer": "return=representation"},
    )
    payload = _json(status, body)
    staging = payload.get("msdyn_projectimportstagingid")
    if not isinstance(staging, str):
        raise MppFillError("Planner did not open an import row")
    return staging


def _upload(org: str, token: str, staging: str, filename: str, data: bytes) -> None:
    status, body = _call(
        "PATCH",
        f"{org}/api/data/v9.2/msdyn_projectimportstagings({staging})/msdyn_importedmppfile",
        token,
        data,
        {"Content-Type": "application/octet-stream", "x-ms-file-name": Path(filename).name},
    )
    if status not in (204, 200):
        raise MppFillError(_http_error(status, body))


def _import(org: str, token: str, staging: str) -> tuple[str, dict]:
    status, body = _call(
        "POST",
        f"{org}/api/data/v9.2/msdyn_projectimportstagings({staging})/Microsoft.Dynamics.CRM.msdyn_ImportProjectV2",
        token,
        b"{}",
        {"Content-Type": "application/json"},
    )
    payload = _json(status, body)
    project_id, import_status, warnings = parse_import(payload)
    if import_status != 201 or not _GUID.match(project_id):
        raise MppFillError(failure_message(payload))
    return project_id, warnings


def failure_message(payload: dict) -> str:
    """Text for a failed ImportProjectV2 response. No token and no long dump."""
    raw = payload.get("Result") or "{}"
    try:
        result = json.loads(raw) if isinstance(raw, str) else raw
    except json.JSONDecodeError:
        result = None
    message = ""
    status = -1
    if isinstance(result, dict):
        message = str(result.get("message") or "").replace("\n", " ").strip()
        if isinstance(result.get("importStatus"), int):
            status = result["importStatus"]
    if len(message) > 300:
        message = message[:300]
    if message:
        return message
    return f"Planner did not import the file (status {status})"


def schedule_ready(finish: str) -> bool:
    """The schedule service fills the project finish a few seconds after import."""
    return bool(finish)


def _wait_for_schedule(org: str, token: str, project_id: str) -> None:
    deadline = time.monotonic() + 30
    start = finish = ""
    lines: list[str] = []
    while True:
        start, finish = _read_schedule(org, token, project_id)
        lines = _report_tasks(org, token, project_id)
        if schedule_ready(finish) or time.monotonic() >= deadline:
            break
        time.sleep(2)
    if schedule_ready(finish) and not lines and time.monotonic() < deadline:
        time.sleep(2)
        lines = _report_tasks(org, token, project_id)
    if start or finish:
        print(f"schedule {start} – {finish}")
    for line in lines:
        print(line)


def _read_schedule(org: str, token: str, project_id: str) -> tuple[str, str]:
    status, body = _call(
        "GET",
        f"{org}/api/data/v9.2/msdyn_projects({project_id})?$select=msdyn_scheduledstart,msdyn_finish",
        token,
    )
    if status != 200:
        return "", ""
    try:
        payload = json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return "", ""
    if not isinstance(payload, dict):
        return "", ""
    return str(payload.get("msdyn_scheduledstart") or ""), str(payload.get("msdyn_finish") or "")


# Names from docs/PLANNER-IMPORT.md. A longer subject still matches on this prefix.
_CHECK_PREFIXES = (
    "controles en correcties administratie",
    "afsluiting van de administratie",
    "controle op de boekingen.",
    "de eerste verwerking van memoboekingen",
    "de laatste memoboekingen",
)


def check_lines(rows: list) -> list[str]:
    """Start and finish for the tasks the working import is checked against."""
    lines: list[str] = []
    seen: set[str] = set()
    for row in rows:
        if not isinstance(row, dict):
            continue
        subject = str(row.get("msdyn_subject") or "")
        key = subject.casefold()
        prefix = next((item for item in _CHECK_PREFIXES if key.startswith(item)), "")
        if not prefix or prefix in seen:
            continue
        seen.add(prefix)
        start = row.get("msdyn_scheduledstart") or ""
        finish = row.get("msdyn_scheduledend") or ""
        lines.append(f"{subject}: {start} – {finish}")
    return lines


def _report_tasks(org: str, token: str, project_id: str) -> list[str]:
    query = urllib.parse.quote(f"_msdyn_project_value eq {project_id}")
    status, body = _call(
        "GET",
        f"{org}/api/data/v9.2/msdyn_projecttasks?$select=msdyn_subject,msdyn_scheduledstart,msdyn_scheduledend&$filter={query}",
        token,
    )
    if status != 200:
        return []
    try:
        payload = json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return []
    rows = payload.get("value") if isinstance(payload, dict) else None
    if not isinstance(rows, list):
        return []
    return check_lines(rows)


def _call(method: str, url: str, token: str, data: bytes | None = None, headers: dict | None = None) -> tuple[int, bytes]:
    request = urllib.request.Request(url, data=data, method=method)
    request.add_header("Authorization", f"Bearer {token}")
    request.add_header("Accept", "application/json")
    request.add_header("OData-MaxVersion", "4.0")
    request.add_header("OData-Version", "4.0")
    for key, value in (headers or {}).items():
        request.add_header(key, value)
    try:
        with urllib.request.urlopen(request, timeout=180) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as exc:
        detail = exc.read()
        return exc.code, detail
    except urllib.error.URLError as exc:
        raise MppFillError(f"cannot reach Planner ({exc.reason})") from exc


def _json(status: int, body: bytes) -> dict:
    if status not in (200, 201):
        raise MppFillError(_http_error(status, body))
    try:
        payload = json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise MppFillError("Planner returned a response that is not JSON") from exc
    if not isinstance(payload, dict):
        raise MppFillError("Planner returned a response that is not a record")
    return payload


def _http_error(status: int, body: bytes) -> str:
    message = ""
    try:
        payload = json.loads(body.decode("utf-8"))
        error = payload.get("error") if isinstance(payload, dict) else None
        if isinstance(error, dict):
            message = str(error.get("message") or "")
    except (UnicodeDecodeError, json.JSONDecodeError):
        message = ""
    message = message.replace("\n", " ").strip()
    if len(message) > 300:
        message = message[:300]
    if message:
        return f"Planner returned {status}: {message}"
    return f"Planner returned {status}"
