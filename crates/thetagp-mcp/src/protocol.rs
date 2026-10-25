//! The JSON-RPC a model host speaks, over the standard streams.
//!
//! One message per line, as the stdio transport of the Model Context Protocol
//! carries them: a request is answered with a result or an error, and a
//! notification -- a message with no id -- is answered with nothing.
//!
//! The tools are executed by a backend: the device in this process, or the
//! daemon that owns it. Either way the answers a host reads are the same.

use serde_json::{Value as Json, json};

use thetagp_tools as tools;

/// The protocol revision this server speaks.
pub const REVISION: &str = "2024-11-05";

/// What the tools are asked of.
pub trait Backend {
    fn call(&mut self, name: &str, arguments: &Json) -> Result<Json, Box<dyn std::error::Error>>;
}

/// The device in this process: the shape a test and a one-off run take.
impl<T: thetagp_client::Transport> Backend for thetagp_client::Session<T> {
    fn call(&mut self, name: &str, arguments: &Json) -> Result<Json, Box<dyn std::error::Error>> {
        tools::call(self, name, arguments)
    }
}

/// The daemon that owns the device: the shape a window and a model share.
impl Backend for thetagp_ipc::Client {
    fn call(&mut self, name: &str, arguments: &Json) -> Result<Json, Box<dyn std::error::Error>> {
        thetagp_ipc::Client::call(
            self,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
        .map_err(|error| error.into())
    }
}

/// The answer to one message, or `None` when it was a notification.
pub fn handle<B: Backend>(backend: &mut B, request: &Json) -> Option<Json> {
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
            match backend.call(name, &arguments) {
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
