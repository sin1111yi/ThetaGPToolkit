//! The daemon: the one process that owns the device.
//!
//! A device is one port, and two programs that open it take it from each other.
//! This process is where the port lives instead: it holds the connection, and
//! the window, the MCP server and anything else ask it over a local socket.
//!
//! One session is answered through two surfaces: `tools/*`, the model's view —
//! the same calls the MCP server carries over stdio — and `session/*`, the typed
//! one the window reads, which hands back the values rather than their text.

use std::net::TcpListener;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value as Json, json};

use thetagp_client::{SerialTransport, Session, Transport, Value, sim};

type Link = Box<dyn Transport + Send>;

/// The name a connection takes for a device that answers in this process.
pub const SIMULATED: &str = "simulated";

/// An open connection: where it came from, and the session itself.
struct Open {
    source: String,
    session: Session<Link>,
}

/// The device, and how long an exchange waits.
pub struct Service {
    session: Mutex<Option<Open>>,
    timeout: Duration,
}

impl Service {
    pub fn new(timeout: Duration) -> Service {
        Service {
            session: Mutex::new(None),
            timeout,
        }
    }

    /// Opens what `source` names: a port's path, `simulated`, or nothing, which
    /// asks the ports in turn.
    ///
    /// A source that does not answer is not taken, and the connection stays as
    /// it was: no session is held that the device did not confirm.
    pub fn connect(&self, source: Option<&str>) -> Result<String, String> {
        let (name, link) = match source {
            Some(SIMULATED) => (
                SIMULATED.to_string(),
                Box::new(sim::device()) as Link,
            ),
            Some(port) => (
                port.to_string(),
                Box::new(SerialTransport::open(port).map_err(|error| format!("{port}: {error}"))?)
                    as Link,
            ),
            None => {
                let port = ask_the_ports()?;
                let link = Box::new(
                    SerialTransport::open(&port).map_err(|error| format!("{port}: {error}"))?,
                ) as Link;
                (port, link)
            }
        };

        let mut session = Session::new(link, self.timeout);
        session
            .ping()
            .map_err(|error| format!("{name}: {error}"))?;

        if let Ok(mut held) = self.session.lock() {
            *held = Some(Open {
                source: name.clone(),
                session,
            });
        }
        Ok(name)
    }

    /// Drops the connection; the next call finds nothing to answer with.
    pub fn disconnect(&self) {
        if let Ok(mut held) = self.session.lock() {
            *held = None;
        }
    }

    /// Where the connection came from, when one is open.
    pub fn state(&self) -> Json {
        match self.session.lock() {
            Ok(held) => match held.as_ref() {
                Some(open) => json!({ "connected": true, "source": open.source }),
                None => json!({ "connected": false, "source": Json::Null }),
            },
            Err(_) => json!({ "connected": false, "source": Json::Null }),
        }
    }

    /// The answer to one message, or `None` when it was a notification.
    pub fn answer(&self, request: &Json) -> Option<Json> {
        let id = request.get("id").cloned();
        id.as_ref()?;

        let method = request
            .get("method")
            .and_then(Json::as_str)
            .unwrap_or_default();

        // The parameters a method may carry, read once: the arms below answer
        // with what they need and never reach back into the message.
        let named_key = request.pointer("/params/key").and_then(Json::as_str);
        let carried_value = request.pointer("/params/value").cloned();
        let profile = request.pointer("/params/id").and_then(Json::as_u64);
        let copied_from = request.pointer("/params/from").and_then(Json::as_u64);
        let snapshot_text = request.pointer("/params/snapshot").and_then(Json::as_str);

        let reply: Result<Json, (String, i64)> = match method {
            "daemon.version" => Ok(json!({ "version": env!("CARGO_PKG_VERSION") })),
            "session.state" => Ok(self.state()),
            "session.connect" => {
                let source = request.pointer("/params/source").and_then(Json::as_str);
                self.connect(source)
                    .map(|name| json!({ "source": name }))
                    .map_err(|error| (error, -32000))
            }
            "session.disconnect" => {
                self.disconnect();
                Ok(json!({}))
            }
            "session.version" => self.with(|session| {
                let named = session.fw_version().map_err(|error| error.to_string())?;
                Ok(json!({
                    "board": named.board,
                    "version": named.version,
                    "build_date": named.build_date,
                    "build_time": named.build_time,
                }))
            }),
            "session.keys" => self.with(|session| {
                let space = session.key_space().map_err(|error| error.to_string())?;
                Ok(json!(space.keys()))
            }),
            "session.config" => self.with(|session| {
                let config =
                    thetagp_client::Config::read(session).map_err(|error| error.to_string())?;
                Ok(json!(config.values()))
            }),
            "session.set_key" => self.set_key(named_key, carried_value),
            "session.save" => self.with(|session| {
                let saved = session.config_save().map_err(|error| error.to_string())?;
                Ok(json!({
                    "persisted": saved.persisted,
                    "dropped_keys": saved.dropped_keys,
                }))
            }),
            "session.inputs" => self.with(|session| {
                let space = session.key_space().map_err(|error| error.to_string())?;
                Ok(json!({ "inputs": space.inputs() }))
            }),
            "session.profiles" => self.with(|session| {
                let listed = session.profiles().map_err(|error| error.to_string())?;
                let profiles: Vec<Json> = listed
                    .profiles
                    .iter()
                    .map(|entry| json!({ "id": entry.id, "active": entry.active }))
                    .collect();
                Ok(json!({ "active": listed.active, "profiles": profiles }))
            }),
            "session.select" => match profile {
                Some(id) => self.select_profile(id),
                None => Err(("no profile was named".to_string(), -32602)),
            },
            "session.profile_create" => self.create_profile(copied_from),
            "session.profile_delete" => match profile {
                Some(id) => self.delete_profile(id),
                None => Err(("no profile was named".to_string(), -32602)),
            },
            "session.snapshot" => self.with(|session| {
                let snapshot =
                    thetagp_client::Snapshot::take(session).map_err(|error| error.to_string())?;
                let text = snapshot.to_json().map_err(|error| error.to_string())?;
                Ok(json!(text))
            }),
            "session.restore" => match snapshot_text {
                Some(text) => self.restore(text),
                None => Err(("no snapshot was carried".to_string(), -32602)),
            },
            "session.load" => self.with(|session| {
                session.config_load().map_err(|error| error.to_string())?;
                Ok(json!({}))
            }),
            "tools/list" => Ok(thetagp_tools::catalogue()),
            "tools/call" => {
                let name = request
                    .pointer("/params/name")
                    .and_then(Json::as_str)
                    .unwrap_or_default();
                let arguments = request
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let mut held = match self.session.lock() {
                    Ok(held) => held,
                    Err(_) => return None,
                };
                let Some(open) = held.as_mut() else {
                    return Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32002, "message": "nothing is connected" },
                    }));
                };
                match thetagp_tools::call(&mut open.session, name, &arguments) {
                    Ok(result) => Ok(result),
                    // A tool that failed is an answer with the failure in it, not
                    // a protocol error: the caller reads it and decides.
                    Err(error) => Ok(json!({
                        "content": [{ "type": "text", "text": error.to_string() }],
                        "isError": true,
                    })),
                }
            }
            other => Err((format!("no method named {other}"), -32601)),
        };

        Some(match reply {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((message, code)) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": code, "message": message },
            }),
        })
    }

    /// Writes one value, held to the key the device declares.
    fn set_key(
        &self,
        name: Option<&str>,
        value: Option<Json>,
    ) -> Result<Json, (String, i64)> {
        let name = name.ok_or(("no key was named".to_string(), -32602))?;
        let carried = value.ok_or(("no value was carried".to_string(), -32602))?;
        let value: Value = serde_json::from_value(carried)
            .map_err(|error| (format!("the value did not read: {error}"), -32602))?;
        self.with(|session| {
            let space = session.key_space().map_err(|error| error.to_string())?;
            space
                .check(name, &value)
                .map_err(|reject| reject.to_string())?;
            session
                .write_value(name, &value)
                .map_err(|error| error.to_string())?;
            Ok(json!(value))
        })
    }

    /// Makes a profile the one the device runs from.
    fn select_profile(&self, id: u64) -> Result<Json, (String, i64)> {
        self.with(|session| {
            let selected = session
                .profile_select(id as u32)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "active": selected.id }))
        })
    }

    /// Writes a new profile from the body of the one named, or of the one in
    /// effect when none is.
    fn create_profile(&self, from: Option<u64>) -> Result<Json, (String, i64)> {
        self.with(|session| {
            let source = match from {
                Some(id) => id as u32,
                None => session
                    .profiles()
                    .map_err(|error| error.to_string())?
                    .active,
            };
            let body = session
                .profile_get(source)
                .map_err(|error| error.to_string())?;
            let id = session
                .profile_create(&body)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "id": id, "from": source }))
        })
    }

    /// Deletes a profile the store carries; the factory profile is the device's
    /// own and its refusal comes back as the device states it.
    fn delete_profile(&self, id: u64) -> Result<Json, (String, i64)> {
        self.with(|session| {
            session
                .profile_delete(id as u32)
                .map_err(|error| error.to_string())?;
            Ok(json!({}))
        })
    }

    /// Puts a snapshot's settings onto the device, holding each to its keys.
    fn restore(&self, text: &str) -> Result<Json, (String, i64)> {
        let snapshot = thetagp_client::Snapshot::from_json(text)
            .map_err(|error| (format!("the snapshot did not read: {error}"), -32602))?;
        self.with(|session| {
            let restored = snapshot.restore(session).map_err(|error| error.to_string())?;
            Ok(json!({
                "values_written": restored.values_written,
                "profiles_written": restored.profiles_written,
                "keys_unknown": restored.keys_unknown,
                "keys_refused": restored.keys_refused,
                "profiles_refused": restored.profiles_refused,
            }))
        })
    }

    /// Runs `body` against the open session, or says nothing is connected.
    fn with<T>(
        &self,
        body: impl FnOnce(&mut Session<Link>) -> Result<T, String>,
    ) -> Result<T, (String, i64)> {
        let mut held = self
            .session
            .lock()
            .map_err(|_| ("the device is in an unusable state".to_string(), -32603))?;
        let open = held
            .as_mut()
            .ok_or(("nothing is connected".to_string(), -32002))?;
        body(&mut open.session).map_err(|error| (error, -32000))
    }
}

/// The ports are asked rather than guessed at: a port that answers is a device,
/// and one that does not is only a port.
fn ask_the_ports() -> Result<String, String> {
    for candidate in
        thetagp_client::transport::available_ports().map_err(|error| error.to_string())?
    {
        let Ok(transport) = SerialTransport::open(&candidate) else {
            continue;
        };
        let mut probe = Session::new(
            Box::new(transport) as Link,
            Duration::from_millis(250),
        );
        if probe.ping().is_ok() {
            return Ok(candidate);
        }
    }
    Err("no port answered as a ThetaGP".into())
}

/// Serves the socket until it is closed, one thread per client.
pub fn serve(listener: TcpListener, token: String, service: std::sync::Arc<Service>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let service = std::sync::Arc::clone(&service);
        let token = token.clone();
        std::thread::spawn(move || serve_connection(stream, token, service));
    }
}

/// Answers one client until its socket closes.
pub fn serve_connection(
    stream: std::net::TcpStream,
    token: String,
    service: std::sync::Arc<Service>,
) {
    use std::io::{BufRead, BufReader, Write};

    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;
    let mut authorised = false;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }

        let answer = match serde_json::from_str::<Json>(&line) {
            Ok(request) => answer_greeted(&request, &mut authorised, &token, &service),
            Err(error) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": format!("the message did not read: {error}") },
            })),
        };
        let Some(answer) = answer else { continue };
        if writeln!(writer, "{answer}").is_err() || writer.flush().is_err() {
            return;
        }
    }
}

/// Holds the greeting in front of everything else: no method is answered before
/// the token this connection was opened with has been given back.
fn answer_greeted(
    request: &Json,
    authorised: &mut bool,
    token: &str,
    service: &Service,
) -> Option<Json> {
    let id = request.get("id").cloned();
    id.as_ref()?;
    let method = request
        .get("method")
        .and_then(Json::as_str)
        .unwrap_or_default();

    if method == "daemon.hello" {
        let carried = request
            .pointer("/params/token")
            .and_then(Json::as_str)
            .unwrap_or_default();
        let result = if carried == token {
            *authorised = true;
            json!({ "version": env!("CARGO_PKG_VERSION"), "ready": true })
        } else {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32001, "message": "the token does not match" },
            }));
        };
        return Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    if !*authorised {
        return Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32001, "message": "say hello with the token first" },
        }));
    }

    service.answer(request)
}
