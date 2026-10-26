//! The server's answers, held against a device that answers in the process.

use std::time::Duration;

use serde_json::{Value as Json, json};
use thetagp_client::Session;
use thetagp_client::sim;

use thetagp_mcp::protocol;

fn session() -> Session<sim::SimulatedDevice> {
    Session::new(sim::device(), Duration::from_millis(50))
}

/// The result of one call, panicking when the message failed at the protocol.
fn result(session: &mut Session<sim::SimulatedDevice>, request: Json) -> Json {
    let answer = protocol::handle(session, &request).expect("a request is answered");
    assert!(answer.get("error").is_none(), "{answer}");
    answer["result"].clone()
}

/// The text of a tool's answer.
fn text_of(session: &mut Session<sim::SimulatedDevice>, name: &str, arguments: Json) -> String {
    let result = call(session, name, arguments);
    result["content"][0]["text"]
        .as_str()
        .expect("a tool answers with text")
        .to_string()
}

fn call(session: &mut Session<sim::SimulatedDevice>, name: &str, arguments: Json) -> Json {
    result(
        session,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        }),
    )
}

#[test]
fn the_server_names_itself_and_its_revision() {
    let mut session = session();
    let result = result(
        &mut session,
        json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize" }),
    );
    assert_eq!(result["protocolVersion"], protocol::REVISION);
    assert_eq!(result["serverInfo"]["name"], "thetagp");
}

#[test]
fn a_notification_is_answered_with_nothing() {
    let mut session = session();
    let answer = protocol::handle(
        &mut session,
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    );
    assert!(answer.is_none());
}

#[test]
fn the_tools_are_declared_with_their_arguments() {
    let mut session = session();
    let result = result(
        &mut session,
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
    );
    let tools = result["tools"].as_array().unwrap();
    assert!(tools.len() >= 20, "{} tools", tools.len());

    let set = tools.iter().find(|tool| tool["name"] == "set_key").unwrap();
    let required = set["inputSchema"]["required"].as_array().unwrap();
    assert!(required.iter().any(|name| name == "key"));
    assert!(required.iter().any(|name| name == "value"));
}

#[test]
fn the_device_tools_answer_from_the_device() {
    let mut session = session();

    let version = text_of(&mut session, "version", json!({}));
    assert!(version.contains("BoringTechH743"), "{version}");

    let keys = text_of(&mut session, "keys", json!({}));
    assert!(keys.contains("led.rgb.hz"), "{keys}");

    let usage = text_of(&mut session, "usage", json!({}));
    assert!(usage.contains("cpu_load_percent"), "{usage}");
}

#[test]
fn a_key_is_read_and_written() {
    let mut session = session();

    let read = text_of(&mut session, "get_key", json!({ "key": "led.bri" }));
    assert_eq!(read, "128");

    let written = text_of(
        &mut session,
        "set_key",
        json!({ "key": "led.bri", "value": 100 }),
    );
    assert!(written.contains("\"in_effect\": true"), "{written}");
    assert_eq!(
        text_of(&mut session, "get_key", json!({ "key": "led.bri" })),
        "100"
    );
}

#[test]
fn a_value_the_key_cannot_hold_is_refused_before_it_travels() {
    let mut session = session();
    let answer = call(
        &mut session,
        "set_key",
        json!({ "key": "led.bri", "value": 999 }),
    );

    assert_eq!(answer["isError"], true);
    let text = answer["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("led.bri"), "{text}");
    // The device still holds what it held.
    assert_eq!(
        text_of(&mut session, "get_key", json!({ "key": "led.bri" })),
        "128"
    );
}

#[test]
fn a_profile_is_written_and_read_back() {
    let mut session = session();
    let written = text_of(
        &mut session,
        "profile_create",
        json!({ "body": "{\"ver\":2,\"led\":{\"bri\":9}}" }),
    );
    assert!(written.contains("\"id\": 1"), "{written}");

    let read = text_of(&mut session, "profile_get", json!({ "id": 1 }));
    assert!(read.contains("led"), "{read}");
}

#[test]
fn a_snapshot_goes_out_and_back_in() {
    let mut session = session();
    let taken = text_of(&mut session, "snapshot", json!({}));
    assert!(taken.contains("BoringTechH743"), "{taken}");

    let restored = text_of(&mut session, "restore", json!({ "snapshot": taken }));
    assert!(restored.contains("\"values_written\": 8"), "{restored}");
}

#[test]
fn the_protocol_tools_reach_the_schema() {
    let mut session = session();

    let arms = text_of(&mut session, "commands", json!({}));
    assert!(arms.contains("sys.ping"), "{arms}");
    assert!(arms.contains("profile.put_chunk"), "{arms}");

    let reply = text_of(
        &mut session,
        "send",
        json!({ "arm": "sys.get_fw_version", "fields": [] }),
    );
    assert!(reply.contains("version"), "{reply}");

    let frame = text_of(&mut session, "decode", json!({ "hex": "0401020304 0a00" }));
    assert!(frame.contains("01 02 03 04"), "{frame}");
}

#[test]
fn a_method_the_server_has_not_is_a_protocol_error() {
    let mut session = session();
    let answer = protocol::handle(
        &mut session,
        &json!({ "jsonrpc": "2.0", "id": 7, "method": "resources/list" }),
    )
    .unwrap();
    assert_eq!(answer["error"]["code"], -32601);
}

#[test]
fn a_tool_the_server_has_not_is_an_answer_that_says_so() {
    let mut session = session();
    let answer = call(&mut session, "no_such_tool", json!({}));
    assert_eq!(answer["isError"], true);
}

#[test]
fn the_reset_asks_the_device_to_restart() {
    let mut session = session();
    let answer = text_of(&mut session, "reset", json!({}));
    assert!(answer.contains("restarting"), "{answer}");
}

#[test]
fn the_check_holds_the_active_profile_against_what_is_in_effect() {
    let mut session = session();

    let created = text_of(
        &mut session,
        "profile_create",
        json!({ "body": "{\"ver\":2,\"map\":{\"socd\":3}}" }),
    );
    let id = serde_json::from_str::<Json>(&created).expect("the answer carries the id")["id"]
        .as_u64()
        .expect("the created profile has an id");
    text_of(&mut session, "profile_select", json!({ "id": id }));

    // A profile that is what the device runs reads as in effect.
    text_of(&mut session, "set_key", json!({ "key": "map.socd", "value": 3 }));
    let checked = text_of(&mut session, "check", json!({}));
    assert!(checked.contains("\"profile_is_in_effect\": true"), "{checked}");
    assert!(checked.contains("\"matching\": 1"), "{checked}");

    // One the device has been told to run from, and is not, names the key.
    text_of(&mut session, "set_key", json!({ "key": "map.socd", "value": 1 }));
    let checked = text_of(&mut session, "check", json!({}));
    assert!(checked.contains("\"profile_is_in_effect\": false"), "{checked}");
    assert!(checked.contains("map.socd"), "{checked}");
}
