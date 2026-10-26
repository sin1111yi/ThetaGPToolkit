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
    /// The name the profile's body carries; empty when it carries none.
    #[serde(default)]
    pub name: String,
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
    pub profiles_refused: Vec<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Sources,
    Keys,
    Remap,
    Profiles,
}

/// What a typed value belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// One key of the configuration, by its row.
    Key(usize),
    /// One slot of the remap table, by its position.
    Remap(usize),
    /// The name of the profile at that position in the list.
    ProfileName(usize),
}

/// A value being typed.
pub struct Editing {
    pub target: Target,
    pub text: String,
    /// Which element of a table the value under the keys is: a key that holds a
    /// run is written one slot at a time, and a scalar keeps the first.
    pub slot: usize,
    /// Whether the next character replaces the element instead of extending it:
    /// an element is under the keys when it opens, so typing picks a value
    /// rather than appending to the one already there.
    pub fresh: bool,
}

impl Editing {
    /// The elements the text holds: a scalar carries one, a table one per slot.
    fn parts(&self) -> Vec<String> {
        self.text
            .split(',')
            .map(|part| part.trim().to_string())
            .collect()
    }

    /// Writes one element and leaves the rest as they were.
    fn write_part(&mut self, at: usize, value: String) {
        let mut parts = self.parts();
        if let Some(part) = parts.get_mut(at) {
            *part = value;
            self.text = parts.join(", ");
        }
    }

    /// The element the keys are in, or the whole value for a scalar.
    fn part(&self) -> String {
        self.parts()
            .get(self.slot)
            .cloned()
            .unwrap_or_default()
    }
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

    /// The inputs the board wires: the remap table's real slots.
    pub inputs: usize,
    pub remap_selected: usize,

    pub profiles: Vec<ProfileEntry>,
    pub profile_selected: usize,
    /// The profile a first `x` asked to delete; a second one deletes it.
    pub pending_delete: Option<u32>,

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
            inputs: 0,
            identity: None,
            keys: Vec::new(),
            values: BTreeMap::new(),
            selected: 0,
            editing: None,
            remap_selected: 0,
            profiles: Vec::new(),
            profile_selected: 0,
            pending_delete: None,
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

        let declared = self.call("session.inputs", json!({}))?;
        self.inputs = declared
            .get("inputs")
            .and_then(Json::as_u64)
            .unwrap_or(0) as usize;

        let config = self.call("session.config", json!({}))?;
        self.values = serde_json::from_value(config).map_err(|error| error.to_string())?;

        let profiles = self.call("session.profiles", json!({}))?;
        let listed: Profiles =
            serde_json::from_value(profiles).map_err(|error| error.to_string())?;
        self.profiles = listed.profiles;

        if self.selected >= self.keys.len() {
            self.selected = self.keys.len().saturating_sub(1);
        }
        if self.remap_selected >= self.remap_len() {
            self.remap_selected = self.remap_len().saturating_sub(1);
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
        // A device that answered has values to read: the keys are where a
        // reader lands, rather than the list of sources they just left.
        self.pane = Pane::Keys;
        self.refresh()
    }

    pub fn disconnect(&mut self) -> Result<(), String> {
        self.call("session.disconnect", json!({}))?;
        self.message = "disconnected".into();
        self.refresh()
    }

    /// The key the remap table is carried in, when the device declares one.
    pub fn remap_key(&self) -> Option<&Key> {
        self.keys
            .iter()
            .find(|key| key.name == "map.btn_map" && matches!(key.kind, KeyType::U8Array))
    }

    /// The slots the remap table holds.
    pub fn remap_values(&self) -> Vec<i32> {
        match self.values.get("map.btn_map") {
            Some(Value::Array(values)) => values.clone(),
            _ => Vec::new(),
        }
    }

    /// The slots the remap pane shows: the inputs this board wires, and no
    /// more. A device that declares no count falls back to the table's length.
    pub fn remap_len(&self) -> usize {
        match self.inputs {
            0 => self
                .remap_key()
                .map(|key| key.count as usize)
                .unwrap_or(0),
            inputs => inputs.min(
                self.remap_key()
                    .map(|key| key.count as usize)
                    .unwrap_or(inputs),
            ),
        }
    }

    /// The target one input slot feeds, as the table carries it.
    pub fn remap_text(&self, slot: usize) -> String {
        match self.remap_values().get(slot) {
            Some(value) => value.to_string(),
            None => String::new(),
        }
    }

    /// Whether the key at that row holds a run rather than one number.
    pub fn is_array(&self, row: usize) -> bool {
        matches!(self.keys.get(row).map(|key| key.kind), Some(KeyType::U8Array))
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

    /// The choices the item in focus offers, by the name each is read by, and
    /// the one it holds now.
    pub fn options(&self) -> (String, Vec<(String, i32)>, Option<i32>) {
        match self.pane {
            Pane::Keys => {
                let Some(key) = self.keys.get(self.selected) else {
                    return (String::new(), Vec::new(), None);
                };
                let current = match self.values.get(&key.name) {
                    Some(Value::Scalar(number)) => Some(*number as i32),
                    Some(Value::Array(run)) => run.first().copied(),
                    None => None,
                };
                (key.name.clone(), choices_of(key), current)
            }
            Pane::Remap => {
                let current = self.remap_text(self.remap_selected).parse::<i32>().ok();
                let choices = self.remap_key().map(choices_of).unwrap_or_default();
                ("map.btn_map".to_string(), choices, current)
            }
            Pane::Profiles => ("profiles".to_string(), Vec::new(), None),
            Pane::Sources => ("sources".to_string(), Vec::new(), None),
        }
    }

    /// How a key's value reads: the name its enum gives, the number beside it.
    pub fn value_display(&self, row: usize) -> String {
        let Some(key) = self.keys.get(row) else {
            return String::new();
        };
        match self.values.get(&key.name) {
            Some(Value::Scalar(number)) => crate::labels::value(key, *number as i32),
            Some(Value::Array(run)) => crate::labels::values(key, run),
            None => String::new(),
        }
    }

    pub fn begin_edit(&mut self) -> Result<(), String> {
        match self.pane {
            Pane::Profiles => {
                // The factory profile is the board's own; its name is not a
                // host's to write, as its body is not.
                let Some(entry) = self.profiles.get(self.profile_selected) else {
                    return Ok(());
                };
                if entry.id == 0 {
                    return Err(
                        "the factory profile is the board's own and is not renamed".into()
                    );
                }
                self.editing = Some(Editing {
                    target: Target::ProfileName(self.profile_selected),
                    text: entry.name.clone(),
                    slot: 0,
                    fresh: false,
                });
            }
            Pane::Remap => {
                let slot = self.remap_selected;
                if slot >= self.remap_len() {
                    return Ok(());
                }
                self.editing = Some(Editing {
                    target: Target::Remap(slot),
                    text: self.remap_text(slot),
                    slot: 0,
                    fresh: true,
                });
            }
            _ => {
                let row = self.selected;
                if row >= self.keys.len() {
                    return Ok(());
                }
                self.editing = Some(Editing {
                    target: Target::Key(row),
                    text: self.value_text(row),
                    slot: 0,
                    fresh: true,
                });
            }
        }
        Ok(())
    }

    /// Writes a character into the element under the keys: the first one
    /// replaces what the element held, and the rest extend it.
    pub fn edit_push(&mut self, character: char) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        if editing.fresh {
            editing.write_part(editing.slot, String::new());
            editing.fresh = false;
        }
        let mut part = editing.part();
        part.push(character);
        editing.write_part(editing.slot, part);
    }

    pub fn edit_pop(&mut self) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        let mut part = editing.part();
        part.pop();
        editing.write_part(editing.slot, part);
        editing.fresh = false;
    }

    /// The elements the value being written has: a table's slots, or one.
    pub fn edit_elements(&self) -> usize {
        let Some(editing) = self.editing.as_ref() else {
            return 1;
        };
        match editing.target {
            Target::Key(row) if self.is_array(row) => {
                (self.keys.get(row).map(|key| key.count).unwrap_or(0) as usize).max(1)
            }
            _ => 1,
        }
    }

    /// Moves the keys to another element of a table, wrapping at its ends.
    pub fn edit_slot(&mut self, by: i32) {
        let count = self.edit_elements();
        if count < 2 {
            return;
        }
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        editing.slot = (editing.slot as i32 + by).rem_euclid(count as i32) as usize;
        editing.fresh = true;
    }

    /// Steps the element under the keys to the next value it may take: the one
    /// beside it in the schema's names, or the number one along its range.
    pub fn edit_step(&mut self, by: i32) {
        if !matches!(
            self.editing.as_ref().map(|editing| editing.target),
            Some(Target::Key(_)) | Some(Target::Remap(_))
        ) {
            return;
        }
        let choices = self.options().1;
        let bounds = self.edited_bounds();
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        let at = editing.slot;
        let held = editing
            .parts()
            .get(at)
            .and_then(|part| part.parse::<i32>().ok());

        let next = if !choices.is_empty() {
            let position =
                held.and_then(|held| choices.iter().position(|(_, number)| *number == held));
            match position {
                Some(position) => {
                    let moved = position as i32 + by;
                    choices[moved.rem_euclid(choices.len() as i32) as usize].1
                }
                None if by < 0 => choices[choices.len() - 1].1,
                None => choices[0].1,
            }
        } else if let Some((min, max)) = bounds {
            (held.unwrap_or(min) + by).clamp(min, max)
        } else {
            return;
        };
        editing.write_part(at, next.to_string());
        editing.fresh = true;
    }

    /// The text under the keys: one element of a table, or the whole value.
    pub fn edit_text(&self) -> String {
        self.editing
            .as_ref()
            .map(|editing| editing.part())
            .unwrap_or_default()
    }

    /// The value the device holds at the element being edited.
    pub fn edit_value(&self) -> Option<i32> {
        let editing = self.editing.as_ref()?;
        match editing.target {
            Target::Key(row) => match self.values.get(&self.keys.get(row)?.name)? {
                Value::Scalar(number) => Some(*number as i32),
                Value::Array(run) => run.get(editing.slot).copied(),
            },
            Target::Remap(slot) => self.remap_values().get(slot).copied(),
            Target::ProfileName(_) => None,
        }
    }

    /// The range the number under the keys is held to, when its key declares
    /// one: the schema's names are preferred over it.
    fn edited_bounds(&self) -> Option<(i32, i32)> {
        match self.editing.as_ref()?.target {
            Target::Key(row) => self.keys.get(row).map(|key| (key.min, key.max)),
            Target::Remap(_) => self.remap_key().map(|key| (key.min, key.max)),
            Target::ProfileName(_) => None,
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

        // A profile's name is its body's: renaming writes the body back.
        if let Target::ProfileName(row) = editing.target {
            let Some(entry) = self.profiles.get(row).cloned() else {
                return Ok(());
            };
            self.call(
                "session.profile_rename",
                json!({ "id": entry.id, "name": editing.text.trim() }),
            )?;
            self.message = format!("profile #{} is named {:?}", entry.id, editing.text.trim());
            return self.refresh();
        }

        // A slot of the remap table is written as the whole table, with the one
        // slot replaced: the device holds a run, not an element.
        if let Target::Remap(slot) = editing.target {
            let mut table = self.remap_values();
            if slot >= table.len() {
                return Err("the table carries no such slot".into());
            }
            table[slot] = editing
                .text
                .trim()
                .parse::<i32>()
                .map_err(|_| format!("{}: not a number", editing.text.trim()))?;
            let answer = self.call(
                "session.set_key",
                json!({ "key": "map.btn_map", "value": Value::Array(table.clone()) }),
            )?;
            let written: Value =
                serde_json::from_value(answer).map_err(|error| error.to_string())?;
            self.values.insert("map.btn_map".into(), written);
            self.message = format!("slot {slot} feeds {}", table[slot]);
            return Ok(());
        }

        let Target::Key(row) = editing.target else {
            return Ok(());
        };
        let Some(key) = self.keys.get(row).cloned() else {
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

    /// Writes a new profile from the body of the one in effect.
    pub fn create_profile(&mut self) -> Result<(), String> {
        let answer = self.call("session.profile_create", json!({}))?;
        let id = answer.get("id").and_then(Json::as_u64).unwrap_or(0);
        self.message = format!("profile #{id} created from the one in effect");
        self.refresh()?;
        self.profile_selected = self
            .profiles
            .iter()
            .position(|entry| entry.id as u64 == id)
            .unwrap_or(0);
        Ok(())
    }

    /// Deletes the profile in the list, once a second `x` confirms it.
    pub fn delete_profile(&mut self) -> Result<(), String> {
        let Some(entry) = self.profiles.get(self.profile_selected).cloned() else {
            return Ok(());
        };
        if entry.id == 0 {
            return Err("the factory profile is the board's own and is not deleted".into());
        }
        if self.pending_delete != Some(entry.id) {
            self.pending_delete = Some(entry.id);
            self.message = format!("press x again to delete profile #{}", entry.id);
            return Ok(());
        }

        self.call("session.profile_delete", json!({ "id": entry.id }))?;
        self.pending_delete = None;
        self.message = format!("profile #{} deleted", entry.id);
        self.refresh()?;
        if self.profile_selected >= self.profiles.len() {
            self.profile_selected = self.profiles.len().saturating_sub(1);
        }
        Ok(())
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
        let profiles = if restored.profiles_refused.is_empty() {
            String::new()
        } else {
            format!("; profiles refused: {:?}", restored.profiles_refused)
        };
        self.message = format!(
            "restored {} values, {} profiles{}{}{}",
            restored.values_written,
            restored.profiles_written,
            profiles,
            if refused.is_empty() {
                String::new()
            } else {
                format!("; not taken: {}", refused.join(", "))
            },
            ""
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


/// The values a key's field can take, as a reader scans them: the ones its enum
/// names, and, while the key's range is short enough to read, the ones it does
/// not name -- a remap slot feeds any index the key accepts, whether or not the
/// schema has a word for it.
fn choices_of(key: &Key) -> Vec<(String, i32)> {
    let mut choices = key
        .value_enum
        .as_deref()
        .map(thetagp_proto::enum_values)
        .unwrap_or_default();
    if choices.is_empty() {
        return choices;
    }

    let span = key.max.saturating_sub(key.min);
    if !(0..=64).contains(&span) {
        return choices;
    }
    for number in key.min..=key.max {
        if !choices.iter().any(|(_, value)| *value == number) {
            choices.push((number.to_string(), number));
        }
    }
    choices.sort_by_key(|(_, number)| *number);
    choices
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
        app.begin_edit().expect("an edit opens");
        app.editing.as_mut().expect("an edit").text = "200".into();
        app.commit_edit().expect("a value the key holds");
        assert_eq!(app.values.get("led.bri"), Some(&Value::Scalar(200)));

        app.begin_edit().expect("an edit opens");
        app.editing.as_mut().expect("an edit").text = "9999".into();
        assert!(app.commit_edit().is_err(), "a value out of range is refused");
    }

    #[test]
    fn a_remap_slot_is_written_as_the_table_it_sits_in() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        let key = app.remap_key().expect("the table the device declares");
        assert!(key.count > 0);

        app.pane = Pane::Remap;
        app.remap_selected = 2;
        app.begin_edit().expect("an edit opens");
        app.editing.as_mut().expect("an edit").text = "9".into();
        app.commit_edit().expect("a slot the key holds");

        let table = app.remap_values();
        assert_eq!(table.get(2), Some(&9));
        app.begin_edit().expect("an edit opens");
        app.editing.as_mut().expect("an edit").text = "999".into();
        assert!(app.commit_edit().is_err(), "a slot out of range is refused");
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
        app.begin_edit().expect("an edit opens");
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
        assert!(drawn.contains("LED Brightness"), "the key is named readably");
        assert!(drawn.contains("led.bri"), "the key's own name is shown beside it");
    }

    #[test]
    fn the_options_are_the_values_the_key_declares() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.pane = Pane::Keys;
        app.selected = app
            .keys
            .iter()
            .position(|key| key.name == "usb.input_mode")
            .expect("the key the device declares");

        let (key, choices, current) = app.options();
        assert_eq!(key, "usb.input_mode");
        assert_eq!(current, Some(14));
        assert!(
            choices.iter().any(|(name, number)| name == "GENERIC" && *number == 14),
            "the mode the device is in is among them: {choices:?}"
        );
    }

    #[test]
    fn a_remap_slot_offers_every_index_the_key_accepts() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.pane = Pane::Remap;

        let (_, choices, _) = app.options();
        assert_eq!(choices.len(), 33, "0..31 and the one that feeds nothing");
        assert!(
            choices.iter().any(|(name, number)| *number == 0 && name == "UP"),
            "the first bit is up"
        );
        assert!(
            choices.iter().any(|(name, number)| *number == 4 && name == "B1"),
            "a face button sits at its own bit"
        );
        assert!(
            choices.iter().any(|(name, number)| *number == 24 && name == "E1"),
            "an extra button keeps its bit"
        );
        assert!(
            choices.iter().all(|(name, _)| name.parse::<i32>().is_err()),
            "every index the table can take has a name now"
        );
        assert!(
            choices
                .iter()
                .any(|(name, number)| *number == 255 && name.eq_ignore_ascii_case("unmapped"))
        );
    }

    #[test]
    fn a_profile_is_created_from_the_one_in_effect_and_deleted() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");

        app.create_profile().expect("a profile is written");
        assert_eq!(app.profiles.len(), 2, "the new profile is listed beside the factory one");
        assert!(app.message.contains("created"), "{}", app.message);

        app.delete_profile().expect("a delete is asked for");
        assert!(app.pending_delete.is_some(), "the first press asks");
        app.delete_profile().expect("the delete is confirmed");
        assert_eq!(app.profiles.len(), 1, "the profile is gone, the factory one stays");
        assert_eq!(app.profiles[0].id, 0);
    }

    #[test]
    fn a_profile_is_named_and_the_name_is_listed() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.create_profile().expect("a profile is written");
        let row = app.profile_selected;

        app.pane = Pane::Profiles;
        app.begin_edit().expect("an edit opens");
        app.editing.as_mut().expect("an edit").text = "Main".into();
        app.commit_edit().expect("the name is written");

        assert_eq!(app.profiles[row].name, "Main");
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

    fn row_of(app: &App, name: &str) -> usize {
        app.keys
            .iter()
            .position(|key| key.name == name)
            .expect("the key is declared")
    }

    #[test]
    fn the_key_a_value_goes_into_is_set_apart_in_its_own_colour() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.selected = row_of(&app, "map.btn_map");
        app.begin_edit().expect("the table is editable");
        app.edit_slot(4);

        assert!(
            screen(&app).contains("key 5 of 8"),
            "the key of the table is named"
        );
        assert_eq!(
            colour_of(&app, "key 5 of 8"),
            Some(crate::ui::HIGHLIGHT),
            "and read in the colour kept for what is checked before typing"
        );
    }

    #[test]
    fn the_profile_a_change_lands_in_is_set_apart_too() {
        let app = {
            let mut app = App::at(daemon_on_sim());
            app.refresh().expect("a state");
            app
        };

        assert!(
            screen(&app).contains("editing #0"),
            "the header names the profile the device runs from"
        );
        assert_eq!(
            colour_of(&app, "editing #0"),
            Some(crate::ui::HIGHLIGHT),
            "in the same colour, so the two read as one kind of fact"
        );
    }

    #[test]
    fn the_upright_keys_walk_the_choices_a_value_offers() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.pane = Pane::Remap;
        app.begin_edit().expect("a slot to edit");

        let opening = app.edit_text();
        app.edit_step(1);
        assert_ne!(app.edit_text(), opening, "a key steps the value");
        app.edit_step(-1);
        assert_eq!(app.edit_text(), opening, "and steps it back");
    }

    #[test]
    fn the_sideways_keys_walk_a_table_slot_by_slot() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.selected = row_of(&app, "map.btn_map");
        app.begin_edit().expect("the table is editable");

        assert_eq!(app.edit_text(), "1", "the first slot is under the keys");
        app.edit_slot(1);
        assert_eq!(app.edit_text(), "2", "the next one carries its own value");
        app.edit_slot(-1);
        assert_eq!(app.edit_text(), "1");
    }

    #[test]
    fn a_typed_digit_replaces_the_element_it_opens_on() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.selected = row_of(&app, "map.btn_map");
        app.begin_edit().expect("the table is editable");
        app.edit_slot(1);
        app.edit_push('9');

        assert_eq!(
            app.edit_text(),
            "9",
            "the first digit is the value, not a digit appended to it"
        );
        app.commit_edit().expect("the table is written");
        assert_eq!(
            app.values.get("map.btn_map"),
            Some(&Value::Array(vec![1, 9, 3, 4, 5, 6, 7, 8])),
            "one slot moved and the rest stood still"
        );
    }

    /// The screen as a terminal holds it: the cells, with what each is drawn in.
    fn buffer_of(app: &App) -> ratatui::buffer::Buffer {
        let backend = ratatui::backend::TestBackend::new(100, 40);
        let mut terminal = ratatui::Terminal::new(backend).expect("a terminal");
        terminal
            .draw(|frame| crate::ui::draw(frame, app))
            .expect("a frame");
        terminal.backend().buffer().clone()
    }

    /// One row of the screen as text.
    fn row_text(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect()
    }

    /// The screen as text, without a terminal to hold it.
    fn screen(app: &App) -> String {
        let buffer = buffer_of(app);
        let mut text = String::new();
        for y in 0..buffer.area.height {
            text.push_str(&row_text(&buffer, y));
            text.push('\n');
        }
        text
    }

    /// The colour the first cell of `text` is drawn in, or none when the screen
    /// does not carry it.
    fn colour_of(app: &App, text: &str) -> Option<ratatui::style::Color> {
        let buffer = buffer_of(app);
        for y in 0..buffer.area.height {
            if let Some(at) = row_text(&buffer, y).find(text) {
                return Some(buffer[(at as u16, y)].fg);
            }
        }
        None
    }

    #[test]
    fn the_value_being_typed_is_drawn_over_the_device() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.pane = Pane::Remap;
        let plain = screen(&app);

        app.begin_edit().expect("a slot to edit");
        app.editing.as_mut().expect("an edit").text = "4".into();
        let boxed = screen(&app);

        assert!(
            boxed.contains("edit · input 00"),
            "the box names the field it holds"
        );
        assert!(
            boxed.contains("↑↓ value · enter write · esc cancel"),
            "and the keys it answers to"
        );
        assert!(
            boxed.contains("UP (0)") && boxed.contains("E8 (31)"),
            "every name the value may take is in it"
        );
        assert!(
            boxed.contains("→ B1 (4)"),
            "the value being typed is the one marked"
        );

        // The box keeps to the device's column: what a value may be is still
        // read on the right while it is typed.
        let choices = |line: &str| line.chars().skip(68).collect::<String>();
        for (at, (left, right)) in plain.lines().zip(boxed.lines()).enumerate() {
            assert_eq!(
                choices(left),
                choices(right),
                "line {at} of the choices column was drawn over"
            );
        }
    }
}
