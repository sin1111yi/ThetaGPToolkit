//! The device, as the terminal holds it.
//!
//! The daemon owns the board; this is one more client of its socket. What it
//! shows is what the device declares -- its keys, their ranges, their values --
//! and what it writes is held to those declarations on the device's side.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use thetagp_client::{Key, KeyType, Value};
use thetagp_ipc::{Client, Endpoint};

/// The source that names a device answering in the daemon's own process.
pub const SIMULATED: &str = "simulated";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub board: String,
    pub version: String,
    pub build_date: String,
    pub build_time: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub id: u32,
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profiles {
    pub active: u32,
    pub profiles: Vec<ProfileEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Restored {
    pub values_written: usize,
    pub profiles_written: usize,
    pub keys_unknown: Vec<String>,
    pub keys_refused: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sources,
    Keys,
    Profiles,
}

/// A value being typed.
pub struct Editing {
    pub row: usize,
    pub text: String,
}

/// Everything one screen shows.
pub struct App {
    client: Option<Client>,
    endpoint: Option<Endpoint>,

    pub sources: Vec<String>,
    pub source: usize,
    pub connected: Option<String>,

    pub identity: Option<Identity>,
    pub keys: Vec<Key>,
    pub values: BTreeMap<String, Value>,
    pub selected: usize,
    pub editing: Option<Editing>,

    pub profiles: Vec<ProfileEntry>,
    pub profile_selected: usize,

    pub snapshot: Option<String>,
    pub pane: Pane,
    pub message: String,
    pub error: Option<String>,
}

impl App {
    pub fn new() -> App {
        App {
            client: None,
            endpoint: None,
            sources: Vec::new(),
            source: 0,
            connected: None,
            identity: None,
            keys: Vec::new(),
            values: BTreeMap::new(),
            selected: 0,
            editing: None,
            profiles: Vec::new(),
            profile_selected: 0,
            snapshot: None,
            pane: Pane::Keys,
            message: String::new(),
            error: None,
        }
    }

    /// A window on an endpoint the caller already holds: what a test takes.
    #[cfg(test)]
    pub fn at(endpoint: Endpoint) -> App {
        let mut app = App::new();
        app.endpoint = Some(endpoint);
        app
    }

    /// Reads the sources there are, and asks the daemon what it holds.
    pub fn start(&mut self) {
        if let Err(error) = self.sources_now() {
            self.error = Some(error);
        }
        if let Err(error) = self.refresh() {
            self.error = Some(error);
        }
        // Nothing to edit until a device answers: the first thing to do is pick
        // the source it answers on.
        if self.connected.is_none() {
            self.pane = Pane::Sources;
        }
    }

    fn sources_now(&mut self) -> Result<(), String> {
        let mut sources = vec![SIMULATED.to_string()];
        sources.extend(
            thetagp_client::transport::available_ports().map_err(|error| error.to_string())?,
        );
        self.sources = sources;
        Ok(())
    }

    /// Re-reads what the daemon holds: its connection, identity, keys, values
    /// and profiles.
    pub fn refresh(&mut self) -> Result<(), String> {
        let state = self.call("session.state", json!({}))?;
        self.connected = state
            .get("source")
            .and_then(Json::as_str)
            .map(str::to_string);

        if self.connected.is_none() {
            self.identity = None;
            self.keys.clear();
            self.values.clear();
            self.profiles.clear();
            self.editing = None;
            return Ok(());
        }

        let identity = self.call("session.version", json!({}))?;
        self.identity = Some(
            serde_json::from_value(identity).map_err(|error| error.to_string())?,
        );

        let keys = self.call("session.keys", json!({}))?;
        self.keys = serde_json::from_value(keys).map_err(|error| error.to_string())?;

        let config = self.call("session.config", json!({}))?;
        self.values = serde_json::from_value(config).map_err(|error| error.to_string())?;

        let profiles = self.call("session.profiles", json!({}))?;
        let listed: Profiles =
            serde_json::from_value(profiles).map_err(|error| error.to_string())?;
        self.profiles = listed.profiles;

        if self.selected >= self.keys.len() {
            self.selected = self.keys.len().saturating_sub(1);
        }
        if self.profile_selected >= self.profiles.len() {
            self.profile_selected = self.profiles.len().saturating_sub(1);
        }
        Ok(())
    }

    pub fn connect(&mut self) -> Result<(), String> {
        let source = self
            .sources
            .get(self.source)
            .cloned()
            .ok_or_else(|| "no source is named".to_string())?;
        let answer = self.call("session.connect", json!({ "source": source }))?;
        let name = answer
            .get("source")
            .and_then(Json::as_str)
            .unwrap_or(&source)
            .to_string();
        self.connected = Some(name.clone());
        self.message = format!("{name} answers");
        self.refresh()
    }

    pub fn disconnect(&mut self) -> Result<(), String> {
        self.call("session.disconnect", json!({}))?;
        self.message = "disconnected".into();
        self.refresh()
    }

    /// The text a value is edited in: a number, or a run of them.
    pub fn value_text(&self, row: usize) -> String {
        let Some(key) = self.keys.get(row) else {
            return String::new();
        };
        match self.values.get(&key.name) {
            Some(Value::Array(values)) => values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            Some(Value::Scalar(value)) => value.to_string(),
            None => String::new(),
        }
    }

    pub fn begin_edit(&mut self) {
        let row = self.selected;
        if row >= self.keys.len() {
            return;
        }
        self.editing = Some(Editing {
            row,
            text: self.value_text(row),
        });
    }

    pub fn edit_push(&mut self, character: char) {
        if let Some(editing) = self.editing.as_mut() {
            editing.text.push(character);
        }
    }

    pub fn edit_pop(&mut self) {
        if let Some(editing) = self.editing.as_mut() {
            editing.text.pop();
        }
    }

    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Writes the value being typed, held to the key it belongs to.
    pub fn commit_edit(&mut self) -> Result<(), String> {
        let Some(editing) = self.editing.take() else {
            return Ok(());
        };
        let Some(key) = self.keys.get(editing.row).cloned() else {
            return Ok(());
        };

        let value = if matches!(key.kind, KeyType::U8Array) {
            let mut run = Vec::new();
            for part in editing.text.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                run.push(
                    part.parse::<i32>()
                        .map_err(|_| format!("{part}: not a number"))?,
                );
            }
            Value::Array(run)
        } else {
            Value::Scalar(
                editing
                    .text
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| format!("{}: not a number", editing.text.trim()))?,
            )
        };

        let answer = self.call(
            "session.set_key",
            json!({ "key": key.name, "value": value }),
        )?;
        let written: Value =
            serde_json::from_value(answer).map_err(|error| error.to_string())?;
        self.values.insert(key.name.clone(), written);
        self.message = format!("{} written", key.name);
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), String> {
        let answer = self.call("session.save", json!({}))?;
        let persisted = answer
            .get("persisted")
            .and_then(Json::as_bool)
            .unwrap_or(false);
        let dropped = answer
            .get("dropped_keys")
            .and_then(Json::as_u64)
            .unwrap_or(0);
        self.message = if persisted {
            if dropped == 0 {
                "saved to the device".into()
            } else {
                format!("saved; the device would not keep {dropped} of the values")
            }
        } else {
            "the device did not write the configuration".into()
        };
        Ok(())
    }

    pub fn load(&mut self) -> Result<(), String> {
        self.call("session.load", json!({}))?;
        self.message = "the stored configuration is back in effect".into();
        self.refresh()
    }

    pub fn select_profile(&mut self) -> Result<(), String> {
        let Some(entry) = self.profiles.get(self.profile_selected).cloned() else {
            return Ok(());
        };
        let answer = self.call("session.select", json!({ "id": entry.id }))?;
        let active = answer
            .get("active")
            .and_then(Json::as_u64)
            .unwrap_or(entry.id as u64);
        self.message = format!("#{active} is the profile the device runs from");
        self.refresh()
    }

    pub fn take_snapshot(&mut self) -> Result<(), String> {
        let answer = self.call("session.snapshot", json!({}))?;
        let text = answer
            .as_str()
            .ok_or_else(|| "the device answered no snapshot".to_string())?
            .to_string();
        self.message = format!("snapshot taken, {} bytes", text.len());
        self.snapshot = Some(text);
        Ok(())
    }

    pub fn restore_snapshot(&mut self) -> Result<(), String> {
        let Some(text) = self.snapshot.clone() else {
            return Err("no snapshot has been taken".into());
        };
        let answer = self.call("session.restore", json!({ "snapshot": text }))?;
        let restored: Restored =
            serde_json::from_value(answer).map_err(|error| error.to_string())?;
        let refused = restored
            .keys_unknown
            .iter()
            .chain(restored.keys_refused.iter())
            .cloned()
            .collect::<Vec<_>>();
        self.message = format!(
            "restored {} values, {} profiles{}",
            restored.values_written,
            restored.profiles_written,
            if refused.is_empty() {
                String::new()
            } else {
                format!("; not taken: {}", refused.join(", "))
            }
        );
        self.refresh()
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
        Ok(self.client.as_mut().expect("the client was just attached"))
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

    fn daemon_on_sim() -> Endpoint {
        let service = Arc::new(thetagp_daemon::Service::new(Duration::from_millis(50)));
        service.connect(Some(SIMULATED)).expect("the device answers");
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a socket");
        let port = listener.local_addr().expect("an address").port();
        let token = thetagp_ipc::new_token();
        let served = token.clone();
        std::thread::spawn(move || thetagp_daemon::serve(listener, served, service));
        Endpoint { port, token }
    }

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
    fn nothing_is_shown_while_nothing_is_connected() {
        let mut app = App::at(daemon_without_a_device());
        app.refresh().expect("a state");
        assert!(app.connected.is_none());
        assert!(app.keys.is_empty());
    }

    #[test]
    fn the_simulated_device_fills_the_screen() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        assert_eq!(app.connected.as_deref(), Some(SIMULATED));
        assert_eq!(app.identity.as_ref().map(|named| named.board.as_str()), Some("BoringTechH743"));
        assert!(app.keys.iter().any(|key| key.name == "led.bri"));
    }

    #[test]
    fn a_value_is_typed_and_written_held_to_its_key() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        let row = app
            .keys
            .iter()
            .position(|key| key.name == "led.bri")
            .expect("the key the device declares");
        app.selected = row;
        app.begin_edit();
        app.editing.as_mut().expect("an edit").text = "200".into();
        app.commit_edit().expect("a value the key holds");
        assert_eq!(app.values.get("led.bri"), Some(&Value::Scalar(200)));

        app.begin_edit();
        app.editing.as_mut().expect("an edit").text = "9999".into();
        assert!(app.commit_edit().is_err(), "a value out of range is refused");
    }

    #[test]
    fn a_saved_value_is_the_one_the_device_keeps() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        let row = app
            .keys
            .iter()
            .position(|key| key.name == "led.bri")
            .expect("the key the device declares");
        app.selected = row;
        app.begin_edit();
        app.editing.as_mut().expect("an edit").text = "77".into();
        app.commit_edit().expect("a value the key holds");
        app.save().expect("a save");
        app.load().expect("the stored configuration is read back");
        assert_eq!(app.values.get("led.bri"), Some(&Value::Scalar(77)));
    }

    #[test]
    fn the_screen_shows_what_the_device_declares() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");

        let backend = ratatui::backend::TestBackend::new(100, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("a terminal");
        terminal
            .draw(|frame| crate::ui::draw(frame, &app))
            .expect("a frame");

        let drawn: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(drawn.contains("BoringTechH743"), "the board is named");
        assert!(drawn.contains("led.bri"), "the keys it declares are shown");
    }

    #[test]
    fn a_snapshot_is_taken_and_restored() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.take_snapshot().expect("a snapshot");
        let text = app.snapshot.clone().expect("the snapshot");
        assert!(text.contains("\"config\""));
        app.restore_snapshot().expect("a restore");
        assert!(app.message.starts_with("restored"), "{}", app.message);
    }
}
