//! The link between the daemon and its clients.
//!
//! One process owns the device, so the others reach it rather than the port: a
//! loopback socket carries one JSON-RPC message per line, and the token that
//! guards it is recorded beside its port in a file only its owner may read. The
//! methods are the tools, so a client asks for what a model asks for.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

/// What a client needs to reach a running daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
}

/// The file the daemon records its endpoint in: the user's runtime directory
/// when the session has one, and the temporary directory otherwise.
pub fn endpoint_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("thetagp").join("daemon.json")
}

/// Records the endpoint, readable by this user alone.
pub fn write_endpoint(endpoint: &Endpoint) -> std::io::Result<()> {
    let path = endpoint_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    serde_json::to_writer(&mut file, endpoint)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

/// Reads the endpoint a running daemon recorded.
pub fn read_endpoint() -> std::io::Result<Endpoint> {
    let path = endpoint_path();
    let text = fs::read_to_string(&path)?;
    serde_json::from_str(text.trim())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// Removes the recorded endpoint, called by a daemon on its way down.
pub fn clear_endpoint() {
    let _ = fs::remove_file(endpoint_path());
}

/// A client of a running daemon.
pub struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next_id: i64,
}

impl Client {
    /// Connects to the daemon a running one recorded, and greets it with the
    /// token from the same file.
    pub fn connect() -> Result<Client, String> {
        let endpoint = read_endpoint()
            .map_err(|error| format!("no daemon is recorded ({}): {error}", endpoint_path().display()))?;
        Client::connect_to(&endpoint)
    }

    /// Connects to a daemon whose endpoint the caller already holds.
    pub fn connect_to(endpoint: &Endpoint) -> Result<Client, String> {
        let stream = TcpStream::connect(("127.0.0.1", endpoint.port))
            .map_err(|error| format!("the daemon at port {}: {error}", endpoint.port))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|error| error.to_string())?;
        let reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
        let mut client = Client { reader, writer: stream, next_id: 1 };
        client.call("daemon.hello", json!({ "token": endpoint.token }))?;
        Ok(client)
    }

    /// Asks the daemon one question and reads its answer.
    pub fn call(&mut self, method: &str, params: Json) -> Result<Json, String> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.writer, "{request}")
            .and_then(|_| self.writer.flush())
            .map_err(|error| format!("writing to the daemon: {error}"))?;

        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .map_err(|error| format!("reading from the daemon: {error}"))?;
        if read == 0 {
            return Err("the daemon closed the connection".into());
        }
        let answer: Json =
            serde_json::from_str(line.trim()).map_err(|error| format!("the daemon's answer: {error}"))?;
        if let Some(error) = answer.get("error") {
            let message = error
                .get("message")
                .and_then(Json::as_str)
                .unwrap_or("the daemon refused the call");
            return Err(message.to_string());
        }
        answer
            .get("result")
            .cloned()
            .ok_or_else(|| "the daemon's answer carried no result".into())
    }
}

/// The token that guards a socket: sixteen bytes from the standard library's
/// own randomness, written as hex.
pub fn new_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut text = String::with_capacity(32);
    for round in 0..2u64 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(round);
        hasher.write_u32(std::process::id());
        text.push_str(&format!("{:016x}", hasher.finish()));
    }
    text
}
