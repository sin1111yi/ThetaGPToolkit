//! The tools a model is given: the device's own commands, and the protocol's.
//!
//! The device tools are built on the client's model of it -- the keys it
//! declares, the configuration it holds, the profiles it carries -- so a value
//! is held to its key before it is written and a profile is a body rather than a
//! run of frames. The protocol tools are what reaches an arm this build has no
//! method for, and what reads a frame's bytes back.

use serde_json::{Value as Json, json};

use thetagp_client::{Session, Snapshot, Transport, Value, dynamic, wire};

type Error = Box<dyn std::error::Error>;

/// The answer of a tool: the text of what it did.
fn text(payload: impl Into<Json>) -> Json {
    let payload: Json = payload.into();
    let text = match payload {
        Json::String(text) => text,
        other => serde_json::to_string_pretty(&other).unwrap_or_default(),
    };
    json!({ "content": [{ "type": "text", "text": text }] })
}

/// One tool's declaration.
fn tool(name: &str, description: &str, properties: Json, required: &[&str]) -> Json {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// Every tool, as the host asks for them.
pub fn catalogue() -> Json {
    let id = json!({ "type": "integer", "description": "The profile's id" });
    json!({ "tools": [
        tool("ping", "Whether a device is there and answering.", json!({}), &[]),
        tool("version", "What the firmware is: the board, the version and the build.", json!({}), &[]),
        tool("usage", "The device's own readings: load, memory, flash and profiles.", json!({}), &[]),
        tool("tasks", "What the scheduler's tasks are doing; one task when a tid is given.", json!({
            "tid": { "type": "integer", "description": "The task to report; every task when left out" },
        }), &[]),
        tool("keypad", "What the keypad driver is doing: its rate, its mat and its timings.", json!({}), &[]),
        tool("flash", "What the flash chip is: how large, how programmed, how erased.", json!({}), &[]),
        tool("keys", "Every configuration key the device declares, with its range and shape.", json!({
            "domain": { "type": "string", "description": "Report only the keys under this domain" },
        }), &[]),
        tool("get_key", "The value one configuration key holds.", json!({
            "key": { "type": "string", "description": "The key's name, as `keys` spells it" },
        }), &["key"]),
        tool("set_key", "Writes one configuration key, in effect until the store is written.", json!({
            "key": { "type": "string", "description": "The key's name" },
            "value": { "description": "One number, or a run of them for an array key" },
        }), &["key", "value"]),
        tool("save_config", "Writes the configuration to the device's storage.", json!({}), &[]),
        tool("load_config", "Reads the stored configuration back into effect.", json!({}), &[]),
        tool("factory_reset", "Puts the configuration back to the defaults the device declares.", json!({}), &[]),
        tool("profiles", "Every profile the store carries, and which of them is active.", json!({}), &[]),
        tool("profile_status", "How the store's flash is accounted for.", json!({}), &[]),
        tool("profile_get", "A profile's body, as the JSON the store keeps.", json!({ "id": id }), &["id"]),
        tool("profile_put", "Writes a body to a profile the store carries; id 0 is the factory profile.", json!({
            "id": id,
            "body": { "type": "string", "description": "The profile's body, as JSON" },
        }), &["id", "body"]),
        tool("profile_create", "Writes a body to a new profile, and reports the id it was given.", json!({
            "body": { "type": "string", "description": "The profile's body, as JSON" },
        }), &["body"]),
        tool("profile_select", "Makes a profile the one the device runs from.", json!({ "id": id }), &["id"]),
        tool("profile_delete", "Deletes a profile.", json!({ "id": id }), &["id"]),
        tool("snapshot", "The whole of what the device is set to: its configuration and every profile.", json!({}), &[]),
        tool("restore", "Writes a snapshot's settings onto the device, holding each to the keys it declares.", json!({
            "snapshot": { "type": "string", "description": "The JSON `snapshot` returned" },
        }), &["snapshot"]),
        tool("enter_dfu", "Asks the device to enter its bootloader, where it waits for an image.", json!({}), &[]),
        tool("commands", "Every arm the protocol schema declares, with its number.", json!({}), &[]),
        tool("send", "Writes one arm by name, with `field=value` for its arguments.", json!({
            "arm": { "type": "string", "description": "The arm's name, as `commands` spells it" },
            "fields": { "type": "array", "items": { "type": "string" }, "description": "`field=value` pairs" },
        }), &["arm"]),
        tool("decode", "Reads a frame's bytes back, without a device.", json!({
            "hex": { "type": "string", "description": "The frame's bytes, in hex" },
        }), &["hex"]),
    ]})
}

/// Runs one tool.
pub fn call<T: Transport>(
    session: &mut Session<T>,
    name: &str,
    arguments: &Json,
) -> Result<Json, Error> {
    let string = |name: &str| -> Result<String, Error> {
        arguments
            .get(name)
            .and_then(Json::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("{name} is missing").into())
    };
    let number = |name: &str| -> Result<u64, Error> {
        arguments
            .get(name)
            .and_then(Json::as_u64)
            .ok_or_else(|| format!("{name} is missing").into())
    };

    Ok(match name {
        "ping" => {
            session.ping()?;
            text("the device answered")
        }
        "version" => {
            let named = session.fw_version()?;
            text(json!({
                "board": named.board,
                "version": named.version,
                "build_date": named.build_date,
                "build_time": named.build_time,
            }))
        }
        "usage" => text(format!("{:#?}", session.usage()?)),
        "tasks" => match arguments.get("tid").and_then(Json::as_i64) {
            Some(tid) => text(format!("{:#?}", session.task_info(tid as i32)?)),
            None => {
                let count = session.usage()?.task_count;
                let mut tasks = Vec::new();
                for tid in 0..count as i32 {
                    match session.task_info(tid) {
                        Ok(task) => tasks.push(format!("{tid}: {task:#?}")),
                        Err(error) => tasks.push(format!("{tid}: {error}")),
                    }
                }
                text(tasks.join("\n\n"))
            }
        },
        "keypad" => text(format!("{:#?}", session.keypad_scan()?)),
        "flash" => text(format!("{:#?}", session.flash_info()?)),
        "keys" => {
            let space = session.key_space()?;
            let keys: Vec<&thetagp_client::Key> =
                match arguments.get("domain").and_then(Json::as_str) {
                    Some(domain) => space.domain(domain),
                    None => space.keys().iter().collect(),
                };
            text(serde_json::to_value(keys)?)
        }
        "get_key" => {
            let key = string("key")?;
            text(serde_json::to_value(Value::from_answer(
                &session.get_key(&key)?,
            ))?)
        }
        "set_key" => {
            let key = string("key")?;
            let value: Value =
                serde_json::from_value(arguments.get("value").cloned().ok_or("value is missing")?)?;
            // The device's own declaration is what the value is held to, so a
            // refusal is named here rather than asked for.
            session.key_space()?.check(&key, &value)?;
            session.write_value(&key, &value)?;
            text(json!({
                "key": key,
                "value": value,
                "in_effect": true,
                "stored": false,
                "note": "save_config writes it to the device's storage",
            }))
        }
        "save_config" => {
            let saved = session.config_save()?;
            text(json!({ "persisted": saved.persisted, "dropped_keys": saved.dropped_keys }))
        }
        "load_config" => {
            session.config_load()?;
            text("the stored configuration is in effect")
        }
        "factory_reset" => {
            session.factory_reset()?;
            text("the configuration is back to its defaults")
        }
        "profiles" => {
            let listed = session.profiles()?;
            let profiles: Vec<Json> = listed
                .profiles
                .iter()
                .map(|entry| json!({ "id": entry.id, "active": entry.active }))
                .collect();
            text(json!({ "active": listed.active, "profiles": profiles }))
        }
        "profile_status" => text(format!("{:#?}", session.profile_status()?)),
        "profile_get" => {
            let id = number("id")? as u32;
            let body = session.profile_get(id)?;
            text(json!({ "id": id, "body": String::from_utf8_lossy(&body) }))
        }
        "profile_put" => {
            let id = number("id")? as u32;
            let body = string("body")?;
            let written = session.profile_put(id, body.as_bytes())?;
            text(json!({ "id": written, "bytes": body.len() }))
        }
        "profile_create" => {
            let body = string("body")?;
            let id = session.profile_create(body.as_bytes())?;
            text(json!({ "id": id, "bytes": body.len() }))
        }
        "profile_select" => {
            let id = number("id")? as u32;
            text(json!({ "active": session.profile_select(id)?.id }))
        }
        "profile_delete" => {
            let id = number("id")? as u32;
            text(json!({ "deleted": session.profile_delete(id)?.id }))
        }
        "snapshot" => {
            let taken = Snapshot::take(session)?;
            text(serde_json::to_value(taken)?)
        }
        "restore" => {
            let taken = Snapshot::from_json(&string("snapshot")?)?;
            let restored = taken.restore(session)?;
            text(json!({
                "values_written": restored.values_written,
                "profiles_written": restored.profiles_written,
                "keys_unknown": restored.keys_unknown,
                "keys_refused": restored.keys_refused,
            }))
        }
        "enter_dfu" => {
            session.enter_dfu()?;
            text("the device is entering its bootloader")
        }
        "commands" => {
            let arms: Vec<Json> = dynamic::arms()?
                .into_iter()
                .map(
                    |arm| json!({ "name": arm.name, "number": arm.number, "carries": arm.carries }),
                )
                .collect();
            text(json!(arms))
        }
        "send" => {
            let arm = string("arm")?;
            let fields: Vec<String> = arguments
                .get("fields")
                .and_then(Json::as_array)
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(Json::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let request = dynamic::build_request(&arm, &fields)?;
            let reply = session.send_request(request)?;
            text(format!("{reply:#?}"))
        }
        "decode" => {
            let bytes = dynamic::parse_hex(&string("hex")?)?;
            let mut decoder = wire::Decoder::new();
            decoder.feed(&bytes, 0);
            let mut read = Vec::new();
            while let Some(payload) = decoder.next_frame() {
                read.push(dynamic::describe(&payload));
            }
            if read.is_empty() {
                read.push("the bytes carry no whole frame".to_string());
            }
            text(read.join("\n"))
        }
        other => return Err(format!("no tool named {other}").into()),
    })
}
