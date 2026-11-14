//! The server: the daemon's tools, on the standard streams.
//!
//! The device is held by one process, so this server reaches it rather than the
//! port: the daemon's socket carries the calls, and the answers are passed back
//! as they are. A client that finds no daemon starts the one beside this binary,
//! so a host that only knows the MCP server still finds the device.

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use serde_json::{Value as Json, json};

#[derive(Parser)]
#[command(
    name = "thetagp-mcp",
    version,
    about = "The ThetaGP device as MCP tools, spoken over stdio"
)]
struct Cli {
    /// The port to talk through; without one, the ports are asked in turn.
    #[arg(long, short, env = "THETAGP_PORT")]
    port: Option<String>,

    /// How long to wait for an answer, in milliseconds.
    #[arg(long, default_value_t = 1000)]
    timeout: u64,

    /// Talk to a device in this process instead of a port.
    #[arg(long)]
    simulated: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut backend = match daemon(&cli) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("thetagp-mcp: {error}");
            return ExitCode::FAILURE;
        }
    };

    // The daemon owns the connection; a flag this server was given is put to it
    // rather than taken as a reason not to start.
    let source = if cli.simulated {
        Some(thetagp_daemon::SIMULATED)
    } else {
        cli.port.as_deref()
    };
    if let Err(error) = backend.call(
        "session.connect",
        json!({ "source": source }),
    ) {
        eprintln!("thetagp-mcp: no device is connected: {error}");
    }

    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }

        let answer = match serde_json::from_str::<Json>(&line) {
            Ok(request) => thetagp_mcp::protocol::handle(&mut backend, &request),
            Err(error) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": format!("the message did not read: {error}") },
            })),
        };

        let Some(answer) = answer else { continue };
        if writeln!(out, "{answer}").is_err() || out.flush().is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

/// Whether the daemon on the socket serves the tools this build carries.
///
/// The tool list is the contract between the two halves, so the list is what
/// the two compare: a daemon built before a tool existed answers every call
/// for it with "no tool named".
fn current(client: &mut thetagp_ipc::Client) -> bool {
    let ours = format!("{:016x}", thetagp_tools::fingerprint());
    match client.call("daemon.version", serde_json::json!({})) {
        Ok(answer) => answer.get("fingerprint").and_then(Json::as_str) == Some(ours.as_str()),
        Err(_) => false,
    }
}

/// The daemon that owns the device, started beside this binary when none runs.
fn daemon(cli: &Cli) -> Result<thetagp_ipc::Client, Box<dyn std::error::Error>> {
    if let Ok(mut client) = thetagp_ipc::Client::connect() {
        if current(&mut client) {
            return Ok(client);
        }
        // The daemon on the socket was built with another tool surface: it is
        // asked to go, and this binary's own daemon takes its place below.
        eprintln!("thetagp-mcp: the daemon serves another build; replacing it");
        let _ = client.call("daemon.shutdown", serde_json::json!({}));
        drop(client);
        std::thread::sleep(Duration::from_millis(250));
    }

    let name = if cfg!(windows) {
        "thetagp-daemon.exe"
    } else {
        "thetagp-daemon"
    };
    let beside = std::env::current_exe()?.with_file_name(name);
    let mut command = std::process::Command::new(&beside);
    // The daemon outlives this server and must not hold the pipes it was
    // started through: a reader of this server's output would wait for an end
    // that never comes. Its own output goes nowhere here; run it by hand to
    // read it.
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.arg("--timeout").arg(cli.timeout.to_string());
    if cli.simulated {
        command.arg("--simulated");
    }
    if let Some(port) = &cli.port {
        command.arg("--port").arg(port);
    }
    command
        .spawn()
        .map_err(|error| format!("starting {}: {error}", beside.display()))?;

    // The daemon records its endpoint once it has a connection; the caller
    // waits for that rather than for a fixed delay.
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(mut client) = thetagp_ipc::Client::connect() {
            if current(&mut client) {
                return Ok(client);
            }
        }
    }
    Err("the daemon did not come up; run thetagp-daemon and read its output".into())
}
