#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::Client;
use serde_json::json;

#[test]
fn task_tool_schemas_advertise_leveling_delay() {
    let c = Client::new();
    let listed = c.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let by_name = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap();

    for name in ["create_task", "update_task"] {
        let props = &by_name(name)["inputSchema"]["properties"];
        assert!(props.get("leveling_delay").is_some(), "tool {name} should expose leveling_delay in schema: {props}");
    }

    let create_items = &by_name("create_tasks")["inputSchema"]["properties"]["tasks"]["items"]["properties"];
    assert!(create_items.get("leveling_delay").is_some(), "tool create_tasks should expose item.leveling_delay in schema: {create_items}");

    let update_items = &by_name("update_tasks")["inputSchema"]["properties"]["updates"]["items"]["properties"];
    assert!(
        update_items.get("leveling_delay").is_some(),
        "tool update_tasks should expose update.leveling_delay in schema: {update_items}"
    );
}

#[test]
fn update_task_sets_and_clears_only_the_target_delay() {
    let c = Client::new();
    c.call("new_project", json!({ "name": "MCP level delay", "start": "2026-12-07" }));
    let prep = c.call("create_task", json!({ "name": "Preparation", "duration": "2d" }))["id"].as_u64().unwrap();
    let send = c.call(
        "create_task",
        json!({ "name": "Send", "duration": "1d", "predecessors": [prep], "constraint_type": "fnlt", "constraint_date": "2026-12-11" }),
    )["id"]
        .as_u64()
        .unwrap();

    c.call("update_task", json!({ "task": prep, "leveling_delay": "3d" }));
    c.call("update_task", json!({ "task": send, "leveling_delay": "2d" }));
    c.call("update_task", json!({ "task": prep, "leveling_delay": "0d" }));

    let prep_task = c.call("get_task", json!({ "task": prep }));
    let send_task = c.call("get_task", json!({ "task": send }));
    assert_eq!(prep_task["leveling_delay_minutes"], 0, "{prep_task}");
    assert_eq!(send_task["leveling_delay_minutes"], 2 * 480, "{send_task}");
    assert_eq!(send_task["predecessors"], "1", "{send_task}");
    assert_eq!(send_task["constraint_type"], "fnlt", "{send_task}");
}

#[test]
fn invalid_leveling_delay_values_are_rejected_and_rolled_back() {
    let c = Client::new();
    c.call("new_project", json!({ "name": "MCP rollback", "start": "2026-12-07" }));
    let task = c.call("create_task", json!({ "name": "Task", "duration": "2d" }))["id"].as_u64().unwrap();
    c.call("update_task", json!({ "task": task, "leveling_delay": "1d" }));

    let e = c.call_err("update_task", json!({ "task": task, "leveling_delay": "3ed" }));
    assert!(e.to_ascii_lowercase().contains("working"), "{e}");
    assert_eq!(c.call("get_task", json!({ "task": task }))["leveling_delay_minutes"], 480);

    let e = c.call_err(
        "update_tasks",
        json!({
            "updates": [
                { "task": task, "notes": "should rollback" },
                { "task": task, "leveling_delay": "-1d" }
            ]
        }),
    );
    assert!(e.contains("update 2"), "{e}");
    let after = c.call("get_task", json!({ "task": task }));
    assert_eq!(after["leveling_delay_minutes"], 480, "{after}");
    assert_eq!(after["notes"], "", "bulk update should roll back note change too: {after}");
}
