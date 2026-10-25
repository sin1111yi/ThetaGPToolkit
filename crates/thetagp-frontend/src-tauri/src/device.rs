//! The device, as the window holds it: the daemon that owns it, and the
//! readings a panel asks it for.
//!
//! A command of the window's is a shell over this, and this is a shell over the
//! daemon's socket: nothing here frames a message, and every key is the device's
//! own declaration rather than a list kept here. The daemon is started beside
//! this binary when none is running, so a window is the only thing a user has to
//! open.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use thetagp_client::{Key, Value};
use thetagp_ipc::{Client, Endpoint};

/// The source that names a device answering in the daemon's own process.
pub const SIMULATED: &str = "simulated";

/// What the window shows about the connection.
#[derive(Clone, Debug, Serialize)]
pub struct ConnState {
    /// Where the session came from, or nothing while none is open.
    pub source: Option<String>,
}

/// The device's own identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub board: String,
    pub version: String,
    pub build_date: String,
    pub build_time: String,
}

/// What writing the configuration did.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Saved {
    pub persisted: bool,
    /// How many values the device would not keep, where it says so.
    pub dropped_keys: Option<u32>,
}

/// The daemon this window asks, attached when the first question is put to it.
pub struct Device {
    client: Option<Client>,
    endpoint: Option<Endpoint>,
}

impl Device {
    pub fn new() -> Device {
        Device {
            client: None,
            endpoint: None,
        }
    }

    /// A device on an endpoint the caller knows: what a test takes.
    #[cfg(test)]
    pub fn at(endpoint: Endpoint) -> Device {
        Device {
            client: None,
            endpoint: Some(endpoint),
        }
    }

    pub fn state(&mut self) -> Result<ConnState, String> {
        let answer = self.call("session.state", json!({}))?;
        Ok(ConnState {
            source: answer
                .get("source")
                .and_then(Json::as_str)
                .map(str::to_string),
        })
    }

    /// Opens what `source` names: a port's path, or `simulated`.
    pub fn connect(&mut self, source: &str) -> Result<ConnState, String> {
        let answer = self.call("session.connect", json!({ "source": source }))?;
        Ok(ConnState {
            source: answer
                .get("source")
                .and_then(Json::as_str)
                .map(str::to_string)
                .or_else(|| Some(source.to_string())),
        })
    }

    pub fn disconnect(&mut self) -> Result<ConnState, String> {
        self.call("session.disconnect", json!({}))?;
        Ok(ConnState { source: None })
    }

    /// The device's identity, board and all.
    pub fn identity(&mut self) -> Result<Identity, String> {
        let answer = self.call("session.version", json!({}))?;
        serde_json::from_value(answer).map_err(|error| error.to_string())
    }

    /// Every key the device declares, in the order it declared them.
    pub fn keys(&mut self) -> Result<Vec<Key>, String> {
        let answer = self.call("session.keys", json!({}))?;
        serde_json::from_value(answer).map_err(|error| error.to_string())
    }

    /// Every key's value, read one at a time.
    pub fn config(&mut self) -> Result<BTreeMap<String, Value>, String> {
        let answer = self.call("session.config", json!({}))?;
        serde_json::from_value(answer).map_err(|error| error.to_string())
    }

    /// Writes one value, held to the key the device declares before it travels.
    pub fn set_key(&mut self, name: &str, value: &Value) -> Result<Value, String> {
        let answer = self.call("session.set_key", json!({ "key": name, "value": value }))?;
        serde_json::from_value(answer).map_err(|error| error.to_string())
    }

    /// Writes the configuration to the device's storage.
    pub fn save(&mut self) -> Result<Saved, String> {
        let answer = self.call("session.save", json!({}))?;
        serde_json::from_value(answer).map_err(|error| error.to_string())
    }

    /// Reads the stored configuration back into effect.
    pub fn load(&mut self) -> Result<(), String> {
        self.call("session.load", json!({}))?;
        Ok(())
    }

    fn call(&mut self, method: &str, params: Json) -> Result<Json, String> {
        self.attach()?.call(method, params)
    }

    fn attach(&mut self) -> Result<&mut Client, String> {
        if self.client.is_none() {
            let client = match &self.endpoint {
                Some(endpoint) => Client::connect_to(endpoint)?,
                None => daemon()?,
            };
            self.client = Some(client);
        }
        Ok(self
            .client
            .as_mut()
            .expect("the client was just attached"))
    }
}

/// The daemon that owns the device, started beside this binary when none runs.
fn daemon() -> Result<Client, String> {
    if let Ok(client) = Client::connect() {
        return Ok(client);
    }

    let name = if cfg!(windows) {
        "thetagp-daemon.exe"
    } else {
        "thetagp-daemon"
    };
    let beside = std::env::current_exe()
        .map_err(|error| error.to_string())?
        .with_file_name(name);
    std::process::Command::new(&beside)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("starting {}: {error}", beside.display()))?;

    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Ok(client) = Client::connect() {
            return Ok(client);
        }
    }
    Err("the daemon did not come up".into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    /// A daemon of this process, holding a simulated device.
    fn daemon_on_sim() -> Endpoint {
        let service = Arc::new(thetagp_daemon::Service::new(Duration::from_millis(50)));
        service
            .connect(Some(SIMULATED))
            .expect("the device answers");
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a socket");
        let port = listener.local_addr().expect("an address").port();
        let token = thetagp_ipc::new_token();
        let served = token.clone();
        std::thread::spawn(move || thetagp_daemon::serve(listener, served, service));
        Endpoint { port, token }
    }

    /// A daemon of this process that holds nothing, until it is asked to.
    fn daemon_without_a_device() -> Endpoint {
        let service = Arc::new(thetagp_daemon::Service::new(Duration::from_millis(50)));
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a socket");
        let port = listener.local_addr().expect("an address").port();
        let token = thetagp_ipc::new_token();
        let served = token.clone();
        std::thread::spawn(move || thetagp_daemon::serve(listener, served, service));
        Endpoint { port, token }
    }

    #[test]
    fn nothing_is_read_while_nothing_is_connected() {
        let mut device = Device::at(daemon_without_a_device());
        assert!(device.state().expect("a state").source.is_none());
        assert!(device.identity().is_err());
        assert!(device.keys().is_err());
        assert!(device.load().is_err());
    }

    #[test]
    fn the_simulated_device_answers_the_window() {
        let mut device = Device::at(daemon_on_sim());
        let state = device.connect(SIMULATED).expect("the device answers");
        assert_eq!(state.source.as_deref(), Some(SIMULATED));

        let identity = device.identity().expect("an identity");
        assert_eq!(identity.board, "BoringTechH743");

        let keys = device.keys().expect("the keys it declares");
        assert!(keys.iter().any(|key| key.name == "led.bri"));

        device
            .set_key("led.bri", &Value::Scalar(200))
            .expect("a value the key holds");
        let config = device.config().expect("the configuration");
        assert_eq!(config.get("led.bri"), Some(&Value::Scalar(200)));
    }

    #[test]
    fn a_value_the_key_cannot_hold_is_refused_before_it_travels() {
        let mut device = Device::at(daemon_on_sim());
        device.connect(SIMULATED).expect("the device answers");
        assert!(device.set_key("led.bri", &Value::Scalar(9999)).is_err());
        assert!(device.set_key("no.such.key", &Value::Scalar(1)).is_err());
        assert!(device
            .config()
            .expect("a configuration")
            .contains_key("led.bri"));
    }

    #[test]
    fn a_source_that_does_not_answer_is_not_taken() {
        let mut device = Device::at(daemon_without_a_device());
        assert!(device.connect("/dev/does-not-exist").is_err());
        assert!(
            device.state().expect("a state").source.is_none(),
            "the window stays disconnected"
        );
    }

    #[test]
    fn a_saved_value_is_the_one_the_device_keeps() {
        let mut device = Device::at(daemon_on_sim());
        device.connect(SIMULATED).expect("the device answers");
        device
            .set_key("led.bri", &Value::Scalar(77))
            .expect("a value the key holds");
        let saved = device.save().expect("a save");
        assert!(saved.persisted, "the device wrote the configuration");

        device.load().expect("the stored configuration is read back");
        let config = device.config().expect("the configuration");
        assert_eq!(config.get("led.bri"), Some(&Value::Scalar(77)));
    }

    #[test]
    fn closing_the_connection_leaves_nothing_readable() {
        let mut device = Device::at(daemon_on_sim());
        device.connect(SIMULATED).expect("the device answers");
        assert!(device.disconnect().expect("a state").source.is_none());
        assert!(device.identity().is_err());
    }
}
