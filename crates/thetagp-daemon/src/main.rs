//! The daemon's entry point: find the socket, record it, and serve it.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;

use thetagp_daemon::{Service, serve};
use thetagp_ipc::Endpoint;

#[derive(Parser)]
#[command(
    name = "thetagp-daemon",
    version,
    about = "The ThetaGP device, held by one process and answered over a local socket"
)]
struct Cli {
    /// The port to open at start; without one, nothing is opened until a client
    /// asks for a connection.
    #[arg(long, short, env = "THETAGP_PORT")]
    port: Option<String>,

    /// How long to wait for an answer, in milliseconds.
    #[arg(long, default_value_t = 1000)]
    timeout: u64,

    /// Open a device in this process instead of a port.
    #[arg(long)]
    simulated: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let service = Arc::new(Service::new(Duration::from_millis(cli.timeout)));

    if cli.simulated || cli.port.is_some() {
        let source = if cli.simulated {
            Some(thetagp_daemon::SIMULATED)
        } else {
            cli.port.as_deref()
        };
        match service.connect(source) {
            Ok(name) => eprintln!("thetagp-daemon: {name} answers"),
            Err(error) => {
                eprintln!("thetagp-daemon: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    let listener = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("thetagp-daemon: no socket: {error}");
            return ExitCode::FAILURE;
        }
    };
    let port = listener
        .local_addr()
        .map(|address| address.port())
        .unwrap_or_default();
    let token = thetagp_ipc::new_token();
    if let Err(error) = thetagp_ipc::write_endpoint(&Endpoint {
        port,
        token: token.clone(),
    }) {
        eprintln!("thetagp-daemon: the endpoint was not recorded: {error}");
        return ExitCode::FAILURE;
    }
    eprintln!("thetagp-daemon: listening on 127.0.0.1:{port}");

    serve(listener, token, Arc::clone(&service));

    thetagp_ipc::clear_endpoint();
    ExitCode::SUCCESS
}
