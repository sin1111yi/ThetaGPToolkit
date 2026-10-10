//! The server: the device's client, on the standard streams.
//!
//! It carries one connection and answers one message at a time, because the
//! device does: a tool call is a request and its reply, and the next message
//! waits for it.

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use serde_json::{Value as Json, json};

use thetagp_client::{SerialTransport, Session, Transport, sim};

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
    let mut session = match open(&cli) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("thetagp-mcp: {error}");
            return ExitCode::FAILURE;
        }
    };

    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }

        let answer = match serde_json::from_str::<Json>(&line) {
            Ok(request) => thetagp_mcp::protocol::handle(&mut session, &request),
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

/// The connection the tools are answered over.
fn open(cli: &Cli) -> Result<Session<Box<dyn Transport>>, Box<dyn std::error::Error>> {
    let timeout = Duration::from_millis(cli.timeout);

    if cli.simulated {
        return Ok(Session::new(Box::new(sim::device()), timeout));
    }

    let port = match &cli.port {
        Some(port) => port.clone(),
        None => {
            // The ports are asked rather than guessed at: a port that answers is
            // a device, and one that does not is only a port.
            let mut found = None;
            for candidate in
                thetagp_client::transport::available_ports().map_err(|error| error.to_string())?
            {
                let Ok(transport) = SerialTransport::open(&candidate) else {
                    continue;
                };
                let mut probe = Session::new(
                    Box::new(transport) as Box<dyn Transport>,
                    Duration::from_millis(250),
                );
                if probe.ping().is_ok() {
                    eprintln!("thetagp-mcp: {candidate} answers");
                    found = Some(candidate);
                    break;
                }
            }
            found.ok_or("no port answered as a ThetaGP; name one with --port")?
        }
    };

    let transport = SerialTransport::open(&port).map_err(|error| format!("{port}: {error}"))?;
    Ok(Session::new(Box::new(transport), timeout))
}
