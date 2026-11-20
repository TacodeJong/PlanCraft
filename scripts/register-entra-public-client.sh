#!/usr/bin/env bash
# One-time helper: create PlanCraft's multi-tenant public client and print the client id.
set -euo pipefail
NAME="${PLANCRAFT_ENTRA_APP_NAME:-PlanCraft Planner export}"
existing="$(az ad app list --display-name "$NAME" --query "[0].appId" -o tsv 2>/dev/null || true)"
if [[ -n "${existing:-}" && "$existing" != "None" ]]; then
  echo "Existing Application (client) ID: $existing"
  echo "Put it in crates/plancraft-app/src/planner_export/defaults.rs as DEFAULT_CLIENT_ID"
  exit 0
fi
app_json="$(az ad app create \
  --display-name "$NAME" \
  --sign-in-audience AzureADMultipleOrgs \
  --is-fallback-public-client true \
  --public-client-redirect-uris "https://login.microsoftonline.com/common/oauth2/nativeclient" \
  -o json)"
app_id="$(printf '%s' "$app_json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["appId"])')"
echo "Created Application (client) ID: $app_id"
echo "Next: Allow public client flows=Yes, add delegated Graph + Dataverse permissions, admin consent, then set DEFAULT_CLIENT_ID."
