//! The JSON-RPC a model host speaks, over the standard streams.
//!
//! One message per line, as the stdio transport of the Model Context Protocol
//! carries them: a request is answered with a result or an error, and a
//! notification -- a message with no id -- is answered with nothing.

use serde_json::{Value as Json, json};

use thetagp_client::{Session, Transport};

use crate::tools;

/// The protocol revision this server speaks.
pub const REVISION: &str = "2024-11-05";

/// The answer to one message, or `None` when it was a notification.
pub fn handle<T: Transport>(session: &mut Session<T>, request: &Json) -> Option<Json> {
    let id = request.get("id").cloned();
    // A message with no id is a notification: it is acted on, if it is one this
    // server acts on, and nothing is sent back for it.
    id.as_ref()?;

    let method = request
        .get("method")
        .and_then(Json::as_str)
        .unwrap_or_default();

    let answer = match method {
        "initialize" => Ok(json!({
            "protocolVersion": REVISION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "thetagp", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools::catalogue()),
        "tools/call" => {
            let name = request
                .pointer("/params/name")
                .and_then(Json::as_str)
                .unwrap_or_default();
            let arguments = request
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match tools::call(session, name, &arguments) {
                Ok(result) => Ok(result),
                // A tool that failed is an answer with the failure in it, not a
                // protocol error: the model is meant to read it and decide.
                Err(error) => Ok(json!({
                    "content": [{ "type": "text", "text": error.to_string() }],
                    "isError": true,
                })),
            }
        }
        other => Err((-32601, format!("no method named {other}"))),
    };

    Some(match answer {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }),
    })
}
