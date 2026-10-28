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

/// How far a sideways key moves a hue, in degrees: about the width of a cell of
/// the band a hue is picked from, so the mark moves with every press.
pub const HUE_STEP: i32 = 10;

/// One line of the options menu at the level it is on: a segment of a key's
/// name that holds more below it, or a key whose name ends here.
///
/// What the device declares is flat -- a key's name carries its own path --
/// so the levels of the menu are the segments those names already have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A segment further segments or keys sit under, and how many keys that is.
    Group { segment: String, keys: usize },
    /// A key, by its row in the device's own declaration.
    Key(usize),
}

/// A question the screen holds until it is answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// A delete it asks about before it is done: a board's profiles are not a
    /// file that can be restored, so what goes is named before it goes.
    Delete { id: u32, name: String },
    /// A save it asks about before the screen goes: the values written to the
    /// device are the running ones, and only a save puts them on the flash.
    Quit,
}

/// A value being typed.
pub struct Editing {
    pub target: Target,
    pub text: String,
    /// Whether the next character replaces the value instead of extending it: a
    /// value is under the keys when the box opens, so typing picks one rather
    /// than appending to the one already there.
    pub fresh: bool,
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
    /// The values the profile holds on the flash: what a change is measured
    /// against when the screen asks whether to save before it goes.
    saved: BTreeMap<String, Value>,
    /// The line of the options menu in focus, at the level it is on.
    pub selected: usize,
    /// The segments the reader has gone into: empty at the top of the menu.
    pub path: Vec<String>,
    pub editing: Option<Editing>,

    /// The inputs the board wires: the remap table's real slots.
    pub inputs: usize,
    pub remap_selected: usize,

    pub profiles: Vec<ProfileEntry>,
    pub profile_selected: usize,
    /// The lines the firmware logged, oldest first: what the device said as it
    /// ran, read off the wire rather than a serial port.
    pub log: Vec<String>,
    /// The last line number the screen holds: the next read starts after it.
    pub log_seq: u32,

    /// The question the screen is asking, if it is asking one.
    pub prompt: Option<Prompt>,

    pub snapshot: Option<String>,
    pub pane: Pane,
    /// Whether the live input view is shown, and what it last read.
    pub show_input: bool,
    pub input: Option<Json>,
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
            saved: BTreeMap::new(),
            selected: 0,
            path: Vec::new(),
            editing: None,
            remap_selected: 0,
            profiles: Vec::new(),
            profile_selected: 0,
            log: Vec::new(),
            log_seq: 0,
            prompt: None,
            snapshot: None,
            pane: Pane::Keys,
            show_input: false,
            input: None,
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
        if let Err(error) = self.refreshed() {
            self.error = Some(error);
        }
        // Nothing to edit until a device answers: the first thing to do is pick
        // the source it answers on.
        if self.connected.is_none() {
            self.pane = Pane::Sources;
        }
        self.poll_log().ok();
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

        // A board with no flash keeps no profiles, and its firmware does not
        // carry the arm that lists them: an empty list is what it has.
        match self.call("session.profiles", json!({})) {
            Ok(profiles) => {
                let listed: Profiles =
                    serde_json::from_value(profiles).map_err(|error| error.to_string())?;
                self.profiles = listed.profiles;
            }
            Err(_) => self.profiles.clear(),
        }

        // A level the device no longer has is one the reader cannot stand on:
        // the path backs out until it names what the device declares.
        while !self.path.is_empty() && self.rows().is_empty() {
            self.path.pop();
        }
        let lines = self.rows().len();
        if self.selected >= lines {
            self.selected = lines.saturating_sub(1);
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
        self.refreshed()?;
        self.poll_log().ok();
        Ok(())
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

    /// The row of the remap table, when the device declares one. The pane below
    /// edits it, one input at a time, so the menu carries it no line.
    pub fn remap_row(&self) -> Option<usize> {
        self.keys
            .iter()
            .position(|key| key.name == "map.btn_map" && matches!(key.kind, KeyType::U8Array))
    }

    /// How many keys the menu carries: the device's own, less the table the
    /// pane below owns.
    pub fn menu_count(&self) -> usize {
        self.keys.len() - usize::from(self.remap_row().is_some())
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

    /// The text a value is edited in: a number, or a run of them.
    /// The input mode the device reports as, as `usb.input_mode` holds it.
    pub fn input_mode(&self) -> Option<i32> {
        match self.values.get("usb.input_mode") {
            Some(Value::Scalar(mode)) => Some(*mode as i32),
            _ => None,
        }
    }

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

    /// The options as the screen lists them: each domain the device's keys fall
    /// into, and the keys inside it when the reader has not closed it.
    ///
    /// A key's name carries its domain, and the device declares the keys of one
    /// domain together, so a domain is a run of the list.
    pub fn rows(&self) -> Vec<Row> {
        let depth = self.path.len();
        let mut rows: Vec<Row> = Vec::new();
        // The keys whose names pass through the segments the reader has gone
        // into, in the order the device declares them: a segment keeps the place
        // of the first key under it, so the menu does not shuffle as it grows.
        for (at, key) in self.keys.iter().enumerate() {
            if Some(at) == self.remap_row() {
                continue;
            }
            let segments: Vec<&str> = key.name.split('.').collect();
            if segments.len() <= depth || segments[..depth] != self.path[..] {
                continue;
            }
            if segments.len() == depth + 1 {
                rows.push(Row::Key(at));
                continue;
            }

            let segment = segments[depth].to_string();
            match rows.last_mut() {
                Some(Row::Group {
                    segment: named,
                    keys,
                }) if *named == segment => *keys += 1,
                _ => rows.push(Row::Group {
                    segment,
                    keys: 1,
                }),
            }
        }
        rows
    }

    /// The key the menu line in focus names, or none when the line is a segment.
    pub fn selected_key(&self) -> Option<usize> {
        match self.rows().get(self.selected)? {
            Row::Key(row) => Some(*row),
            Row::Group { .. } => None,
        }
    }

    /// The segment the menu line in focus names, or none when it names a key.
    pub fn focused_segment(&self) -> Option<(String, usize)> {
        match self.rows().get(self.selected)? {
            Row::Group { segment, keys } => Some((segment.clone(), *keys)),
            Row::Key(_) => None,
        }
    }

    /// `enter` on the menu: a segment is gone into, a key is edited.
    pub fn open_selected(&mut self) -> Result<(), String> {
        let Some((segment, _)) = self.focused_segment() else {
            return self.begin_edit();
        };
        self.path.push(segment);
        self.selected = 0;
        Ok(())
    }

    /// `right` on the menu: the same as `enter` where the line is a segment, and
    /// nothing where it is a key -- a key is edited, not gone into.
    pub fn go_in(&mut self) -> Result<(), String> {
        if self.focused_segment().is_some() {
            self.open_selected()?;
        }
        Ok(())
    }

    /// `left` or `esc`: back out of the level the menu is on, onto the segment
    /// that led into it.
    pub fn go_up(&mut self) -> Result<(), String> {
        let Some(segment) = self.path.pop() else {
            return Ok(());
        };
        self.selected = self
            .rows()
            .iter()
            .position(|row| matches!(row, Row::Group { segment: named, .. } if *named == segment))
            .unwrap_or(0);
        Ok(())
    }

    /// The choices the item in focus offers, by the name each is read by, and
    /// the one it holds now.
    pub fn options(&self) -> (String, Vec<(String, i32)>, Option<i32>) {
        match self.pane {
            Pane::Keys => match self.rows().get(self.selected) {
                Some(Row::Key(row)) => {
                    let Some(key) = self.keys.get(*row) else {
                        return (String::new(), Vec::new(), None);
                    };
                    let current = match self.values.get(&key.name) {
                        Some(Value::Scalar(number)) => Some(*number as i32),
                        Some(Value::Array(run)) => run.first().copied(),
                        None => None,
                    };
                    (key.name.clone(), choices_of(key), current)
                }
                // A segment holds more of the menu rather than a value, so it
                // offers none.
                Some(Row::Group { segment, .. }) => (segment.clone(), Vec::new(), None),
                None => (String::new(), Vec::new(), None),
            },
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
                    fresh: true,
                });
            }
            _ => {
                // A domain's line has no value of its own: `open_selected`
                // folds it before this is reached.
                let Some(row) = self.selected_key() else {
                    return Ok(());
                };
                self.editing = Some(Editing {
                    target: Target::Key(row),
                    text: self.value_text(row),
                    fresh: true,
                });
            }
        }
        Ok(())
    }

    /// Writes a character into the value under the keys: the first one replaces
    /// what it held, and the rest extend it.
    pub fn edit_push(&mut self, character: char) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        if editing.fresh {
            editing.text.clear();
            editing.fresh = false;
        }
        editing.text.push(character);
    }

    pub fn edit_pop(&mut self) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        editing.text.pop();
        editing.fresh = false;
    }

    /// Steps the value under the keys to the next one it may take: the one
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
        let held = editing.text.trim().parse::<i32>().ok();

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
        editing.text = next.to_string();
        editing.fresh = true;
    }

    /// `←`/`→`: along the range of a value nothing names, which is what a value
    /// read off a scale is chosen with. A value with names is stepped by the
    /// upright keys, and is left alone here.
    pub fn edit_nudge(&mut self, by: i32) {
        if !self.options().1.is_empty() {
            return;
        }
        self.edit_step(by);
    }

    /// The hue the profile carries, which the band a saturation is read off is
    /// drawn at.
    pub fn hue_degrees(&self) -> u16 {
        match self.values.get("led.hue") {
            Some(Value::Scalar(number)) => (*number).clamp(0, 360) as u16,
            _ => 0,
        }
    }

    /// The saturation the profile carries for the strip, or full colour when it
    /// carries none: it is what the band a hue is read off is drawn at.
    pub fn saturation(&self) -> u8 {
        match self.values.get("led.sat") {
            Some(Value::Scalar(number)) => (*number).clamp(0, 255) as u8,
            _ => 255,
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
        if persisted {
            // What the flash holds is what the device holds now.
            self.saved = self.values.clone();
        }
        self.message = if persisted {
            if dropped == 0 {
                "saved to the device".into()
            } else {
                format!("saved; the device would not keep {dropped} of the values")
            }
        } else if !self.keeps_profiles() {
            // Nothing to write into: the board carries no profiles.
            "this board keeps no profiles; the values live in it until it restarts".into()
        } else {
            "the device did not write the configuration".into()
        };
        Ok(())
    }

    pub fn load(&mut self) -> Result<(), String> {
        self.call("session.load", json!({}))?;
        self.message = "the stored configuration is back in effect".into();
        self.refreshed()
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
        self.refreshed()
    }

    /// Writes a new profile from the body of the one in effect.
    pub fn create_profile(&mut self) -> Result<(), String> {
        let answer = self.call("session.profile_create", json!({}))?;
        let id = answer.get("id").and_then(Json::as_u64).unwrap_or(0);
        self.message = format!("profile #{id} created from the one in effect");
        self.refreshed()?;
        self.profile_selected = self
            .profiles
            .iter()
            .position(|entry| entry.id as u64 == id)
            .unwrap_or(0);
        Ok(())
    }

    /// Deletes the profile in the list, once a second `x` confirms it.
    /// Reads the lines the firmware has logged since the screen last asked, and
    /// keeps them. A read that fails is not a line of the device's log, so it is
    /// left to the status line.
    /// Reads the input the host's own driver holds, through the daemon.
    pub fn poll_input(&mut self) -> Result<(), String> {
        if self.client.is_none() || !self.show_input {
            return Ok(());
        }
        self.input = Some(self.call("session.input", json!({}))?);
        Ok(())
    }

    pub fn poll_log(&mut self) -> Result<(), String> {
        if self.client.is_none() {
            return Ok(());
        }
        let mut answer = self.call("session.log", json!({ "since": self.log_seq }))?;
        let total = answer.get("total").and_then(Json::as_u64).unwrap_or(0) as u32;
        if total < self.log_seq {
            // The device restarted: its count began again, and the lines the
            // screen holds are from a run that is over. The read that noticed
            // it was asked from a number the new run never reached, so the run
            // is read from its first line.
            self.log.clear();
            self.log_seq = 0;
            answer = self.call("session.log", json!({ "since": 0 }))?;
        }
        let mut newest = self.log_seq;
        if let Some(lines) = answer.get("lines").and_then(Json::as_array) {
            for line in lines {
                let text = line.get("text").and_then(Json::as_str).unwrap_or_default();
                let level = line.get("level").and_then(Json::as_u64).unwrap_or(0);
                let mark = match level {
                    1 => "W ",
                    2 => "E ",
                    3 => "D ",
                    _ => "I ",
                };
                self.log.push(format!("{mark}{}", text.trim_end()));
                newest = line.get("seq").and_then(Json::as_u64).unwrap_or(0) as u32;
            }
        }
        if self.log.len() > 500 {
            let drop = self.log.len() - 500;
            self.log.drain(..drop);
        }
        // A reply carries a few lines, not every one the device holds: the next
        // read starts after the last line this one carried, so the rest follow.
        self.log_seq = newest;
        Ok(())
    }

    /// What the item in focus is, and what it is for: the line the screen reads
    /// beside the device.
    pub fn about(&self) -> (String, String) {
        let answered = |name: &str, title: String| {
            (title, crate::labels::about(name).unwrap_or_default().to_string())
        };
        match self.pane {
            Pane::Keys => match self.rows().get(self.selected) {
                Some(Row::Group { segment, .. }) => {
                    answered(segment, segment.clone())
                }
                Some(Row::Key(row)) => match self.keys.get(*row) {
                    Some(key) => {
                        let title = crate::labels::label(&key.name).unwrap_or(&key.name).to_string();
                        answered(&key.name, title)
                    }
                    None => (String::new(), String::new()),
                },
                None => (String::new(), String::new()),
            },
            Pane::Remap => match self.remap_key() {
                Some(_) => {
                    let slot = self.remap_selected;
                    let button = crate::labels::slot(
                        self.remap_text(slot).parse().unwrap_or(255),
                    );
                    (
                        format!("input {slot:02}"),
                        format!("The button input {slot:02} feeds: {button}."),
                    )
                }
                None => (String::new(), String::new()),
            },
            Pane::Profiles => match self.profiles.get(self.profile_selected) {
                Some(entry) => {
                    let title = format!("#{}", entry.id);
                    let what = if entry.id == 0 {
                        "The profile the board ships with: the file the board falls back to."
                    } else {
                        "A profile this board carries: saving writes the values on screen into it."
                    };
                    (title, what.to_string())
                }
                None => (String::new(), String::new()),
            },
            Pane::Sources => match self.sources.get(self.source) {
                Some(port) => (
                    port.clone(),
                    "A serial port to open the board on. The device is bound once, here.".to_string(),
                ),
                None => (String::new(), String::new()),
            },
        }
    }

    /// Reads the state, and takes what it read as the values the profile holds:
    /// a read that follows a write is not one of these.
    fn refreshed(&mut self) -> Result<(), String> {
        self.refresh()?;
        self.saved = self.values.clone();
        Ok(())
    }

    /// Whether the device has anywhere to keep a change: a board with no flash
    /// carries no profiles, so a save writes nothing and a quit loses nothing.
    pub fn keeps_profiles(&self) -> bool {
        !self.profiles.is_empty()
    }

    /// The keys whose value is not the one the profile holds on the flash.
    pub fn unsaved(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .values
            .iter()
            .filter(|(name, value)| self.saved.get(*name) != Some(value))
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        names
    }

    /// Asks, in a box, whether to save before the screen goes.
    pub fn ask_quit(&mut self) -> Result<(), String> {
        if !self.keeps_profiles() || self.unsaved().is_empty() {
            return Ok(());
        }
        self.prompt = Some(Prompt::Quit);
        Ok(())
    }

    /// Asks, in a box, before deleting the profile in focus.
    pub fn ask_delete(&mut self) -> Result<(), String> {
        let Some(entry) = self.profiles.get(self.profile_selected).cloned() else {
            return Ok(());
        };
        if entry.id == 0 {
            return Err("the factory profile is the board's own and is not deleted".into());
        }
        self.prompt = Some(Prompt::Delete {
            id: entry.id,
            name: entry.name.clone(),
        });
        Ok(())
    }

    /// Deletes the profile the box asks about.
    pub fn confirm_delete(&mut self) -> Result<(), String> {
        let Some(Prompt::Delete { id, .. }) = self.prompt.take() else {
            self.prompt = None;
            return Ok(());
        };

        self.call("session.profile_delete", json!({ "id": id }))?;
        self.message = format!("profile #{id} deleted");
        self.refresh()?;
        if self.profile_selected >= self.profiles.len() {
            self.profile_selected = self.profiles.len().saturating_sub(1);
        }
        Ok(())
    }

    /// Closes the box without answering it, whatever it asked.
    pub fn cancel_prompt(&mut self) {
        self.prompt = None;
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
        self.refreshed()
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
    // An input mode the firmware does not present is not a value a reader may
    // choose: the schema carries it for boards that answer it.
    if key.name == "usb.input_mode" {
        choices.retain(|(_, number)| !crate::labels::unsupported_mode(*number));
    }
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
        select(&mut app, "led.bri");
        app.begin_edit().expect("an edit opens");
        app.editing.as_mut().expect("an edit").text = "100".into();
        app.commit_edit().expect("a value the key holds");
        assert_eq!(app.values.get("led.bri"), Some(&Value::Scalar(100)));

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
        select(&mut app, "led.bri");
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

        let backend = ratatui::backend::TestBackend::new(120, 24);
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
        assert!(drawn.contains("▸ led"), "the menu opens on the keys' segments");
        assert!(drawn.contains("5 keys"), "and says how many each holds");
    }

    #[test]
    fn the_options_are_the_values_the_key_declares() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.pane = Pane::Keys;
        select(&mut app, "usb.input_mode");

        let (key, choices, current) = app.options();
        assert_eq!(key, "usb.input_mode");
        assert_eq!(current, Some(14));
        assert!(
            choices.iter().any(|(name, number)| name == "GENERIC" && *number == 14),
            "the mode the device is in is among them: {choices:?}"
        );
        assert!(
            !choices.iter().any(|(_, number)| *number == 3),
            "a mode the firmware leaves out is not offered: {choices:?}"
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

        app.ask_delete().expect("a delete is asked about");
        assert!(
            app.prompt
                .as_ref()
                .is_some_and(|prompt| *prompt == Prompt::Delete { id: 1, name: String::new() }),
            "the box names the profile it would take"
        );
        app.confirm_delete().expect("the delete is answered");
        assert_eq!(app.profiles.len(), 1, "the profile is gone, the factory one stays");
        assert_eq!(app.profiles[0].id, 0);
    }

    #[test]
    fn a_delete_is_asked_about_in_a_box_before_the_profile_goes() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.create_profile().expect("a profile is written");
        app.pane = Pane::Profiles;
        app.profile_selected = 1;
        let before = app.profiles.len();

        app.ask_delete().expect("a delete is asked about");
        let boxed = screen(&app);
        assert!(
            boxed.contains("delete profile?"),
            "the question is asked in a box"
        );
        assert!(
            boxed.contains("is deleted from the board")
                && boxed.contains("enter delete · esc cancel"),
            "and names both answers"
        );
        assert_eq!(
            app.profiles.len(),
            before,
            "nothing goes until the box is answered"
        );

        app.confirm_delete().expect("the delete is answered");
        assert_eq!(app.profiles.len(), before - 1, "the profile goes");
        assert!(app.prompt.is_none(), "and the box is gone with it");
    }

    #[test]
    fn a_delete_that_is_not_answered_keeps_the_profile() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.create_profile().expect("a profile is written");
        app.pane = Pane::Profiles;
        app.profile_selected = 1;
        let before = app.profiles.len();

        app.ask_delete().expect("a delete is asked about");
        app.cancel_prompt();
        assert!(app.prompt.is_none(), "the box is closed");
        assert_eq!(app.profiles.len(), before, "and the profile stays");
        assert!(
            !screen(&app).contains("delete profile?"),
            "nothing of the box is left on the screen"
        );
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

    /// Puts the menu's focus on a key, by its row in the device's declaration:
    /// the menu is walked down to the level the key sits on first.
    fn select(app: &mut App, name: &str) {
        let key = row_of(app, name);
        let segments: Vec<String> = app.keys[key].name.split('.').map(str::to_string).collect();
        app.path = segments[..segments.len() - 1].to_vec();
        app.selected = app
            .rows()
            .iter()
            .position(|line| *line == Row::Key(key))
            .expect("the key is on the level the menu was walked to");
    }

    /// Where the mark of a band is drawn, and the colours the band is filled
    /// with: what a screen with a colour band holds rather than what it says.
    fn mark_column(app: &App) -> u16 {
        let buffer = buffer_of(app);
        for y in 0..buffer.area.height {
            if let Some(at) = row_text(&buffer, y).find('▲') {
                return at as u16;
            }
        }
        panic!("no mark is on the screen");
    }

    fn band_cells(app: &App) -> Vec<ratatui::style::Color> {
        let buffer = buffer_of(app);
        let mut mark_row = None;
        for y in 0..buffer.area.height {
            if row_text(&buffer, y).contains('▲') {
                mark_row = Some(y);
            }
        }
        let band_row = mark_row.expect("a mark is on the screen") - 1;
        (0..buffer.area.width)
            .map(|x| buffer[(x, band_row)].bg)
            .filter(|colour| *colour != ratatui::style::Color::Reset)
            .collect()
    }

    #[test]
    fn a_hue_is_picked_off_the_wheel_the_strip_draws() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        select(&mut app, "led.hue");
        app.begin_edit().expect("the hue opens");
        app.editing.as_mut().expect("an edit").text = "0".into();

        let cells = band_cells(&app);
        assert_eq!(
            cells.first(),
            Some(&ratatui::style::Color::Rgb(255, 0, 0)),
            "the wheel starts at red"
        );

        let at_left = mark_column(&app);
        app.edit_nudge(HUE_STEP);
        assert_eq!(
            app.editing.as_ref().expect("an edit").text,
            "10",
            "a sideways key steps the value"
        );
        assert!(
            mark_column(&app) > at_left,
            "and the mark moves along with it"
        );
    }

    #[test]
    fn a_saturation_band_runs_from_white_to_the_hue_it_carries() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        assert_eq!(app.hue_degrees(), 180, "the simulated profile is cyan");
        select(&mut app, "led.sat");
        app.begin_edit().expect("the saturation opens");
        app.editing.as_mut().expect("an edit").text = "255".into();

        let cells = band_cells(&app);
        assert_eq!(
            cells.first(),
            Some(&ratatui::style::Color::Rgb(255, 255, 255)),
            "no saturation is white"
        );
        assert_eq!(
            cells.last(),
            Some(&ratatui::style::Color::Rgb(0, 255, 255)),
            "and full saturation is the hue the profile carries"
        );
    }

    #[test]
    fn a_pressed_button_lights_the_input_it_is_wired_to() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        app.show_input = true;
        // East is button 1, and the simulated board's first input feeds it.
        app.input = Some(json!({
            "available": true,
            "name": "a pad",
            "keys": [{ "code": 0x131, "name": "East" }],
            "axes": [],
        }));
        let buffer = buffer_of(&app);

        let mut lit = false;
        for y in 0..buffer.area.height {
            if !row_text(&buffer, y).contains("input 00") {
                continue;
            }
            for x in 0..buffer.area.width {
                if buffer[(x, y)].fg == crate::ui::HIGHLIGHT {
                    lit = true;
                }
            }
        }
        assert!(lit, "the slot feeding the held button is lit");

        // And the button's own cell on the device's side carries the press.
        let mut cell_lit = false;
        for y in 0..buffer.area.height {
            let text = row_text(&buffer, y);
            let Some(at) = text.find("DOWN") else {
                continue;
            };
            if buffer[(at as u16, y)].fg == crate::ui::KEY {
                cell_lit = true;
            }
        }
        assert!(cell_lit, "the button the press names is lit on the left");
    }

    #[test]
    fn the_input_view_shows_what_the_driver_holds() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        app.show_input = true;
        app.input = Some(json!({
            "available": true,
            "name": "a pad",
            "keys": [{ "code": 0x130, "name": "South" }],
            "axes": [
                { "code": 0, "name": "X", "value": 128, "min": 0, "max": 255, "position": 501 }
            ],
        }));
        let buffer = buffer_of(&app);

        let mut text = String::new();
        for y in 0..buffer.area.height {
            text.push_str(&row_text(&buffer, y));
            text.push('\n');
        }
        assert!(text.contains("held"), "the keys the driver holds are named");
        assert!(text.contains("South"), "each held key by its name");
        assert!(text.contains("X "), "and every axis the node carries");
        assert!(text.contains("501"), "with where it stands along its range");
        assert!(
            text.contains("remap (map.btn_map"),
            "and the remap table beside it, where the info column was"
        );
    }

    #[test]
    fn a_key_is_drawn_apart_from_what_it_does() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        let buffer = buffer_of(&app);

        for y in 0..buffer.area.height {
            let line = row_text(&buffer, y);
            let Some(at) = line.find("n new profile") else {
                continue;
            };
            assert_eq!(
                buffer[(at as u16, y)].fg,
                crate::ui::KEY,
                "the key is drawn in the colour kept for one"
            );
            assert_eq!(
                buffer[(at as u16 + 2, y)].fg,
                crate::ui::MUTED,
                "and the words beside it as what it does"
            );
            return;
        }
        panic!("no help line carries `n new profile`");
    }

    #[test]
    fn a_board_with_no_flash_keeps_no_profiles_and_is_not_asked_to_save() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        // The simulated board has a flash; take its profiles away, which is
        // what a device without one answers.
        app.profiles.clear();
        select(&mut app, "led.bri");
        app.begin_edit().expect("the key is editable");
        app.edit_push('9');
        app.commit_edit().expect("the value is written");

        assert!(app.unsaved().len() == 1, "the write is only on the device");
        app.ask_quit().expect("nothing to ask");
        assert!(
            app.prompt.is_none(),
            "a board with nowhere to save is not asked to"
        );
    }

    #[test]
    fn the_log_is_the_lines_the_device_wrote() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        app.poll_log().expect("the device answers");

        assert_eq!(
            app.log,
            vec![
                "I sim: the board is up".to_string(),
                "I sim: nothing is wired to it".to_string()
            ],
            "the lines the device kept, with the level it wrote them at"
        );
        assert_eq!(app.log_seq, 2, "and the number the next read starts after");

        app.poll_log().expect("the device answers again");
        assert_eq!(app.log.len(), 2, "a line already held is not written twice");
    }

    #[test]
    fn a_restarted_device_drops_the_lines_of_the_run_before() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        app.poll_log().expect("the device answers");
        app.log.push("I a line from a run that is over".to_string());
        app.log_seq = 99;

        app.poll_log().expect("the device answers");
        assert!(
            !app.log.iter().any(|line| line.contains("run that is over")),
            "the count began again, so what it held is gone"
        );
        assert_eq!(app.log_seq, 2, "and the new run's lines are read from zero");
    }

    #[test]
    fn the_log_is_drawn_beside_the_device() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        app.poll_log().expect("the device answers");

        let text = screen(&app);
        assert!(text.contains(" log "), "the panel is on the screen: {}", text.lines().count());
        assert!(text.contains("sim: the board is up"), "with what the device said");
    }

    #[test]
    fn a_change_that_is_not_on_the_flash_is_asked_about_before_the_screen_goes() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        assert!(
            app.unsaved().is_empty(),
            "nothing is pending when the screen opens"
        );

        select(&mut app, "led.bri");
        app.begin_edit().expect("the key is editable");
        app.edit_push('9');
        app.commit_edit().expect("the value is written");
        assert_eq!(
            app.unsaved(),
            vec!["led.bri".to_string()],
            "the write is on the device and not on the flash"
        );

        app.ask_quit().expect("the question is put");
        let boxed = screen(&app);
        assert!(boxed.contains("save before quitting?"), "a box asks");
        assert!(
            boxed.contains("1 of the values are not on the flash"),
            "and says how many would be lost"
        );
        assert!(boxed.contains("led.bri"), "and which ones");

        app.cancel_prompt();
        assert!(app.prompt.is_none(), "the box closes on its own answer");
    }

    #[test]
    fn a_save_puts_the_change_on_the_flash_and_leaves_nothing_to_ask() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        select(&mut app, "led.bri");
        app.begin_edit().expect("the key is editable");
        app.edit_push('9');
        app.commit_edit().expect("the value is written");

        app.save().expect("the save");
        assert!(app.unsaved().is_empty(), "the flash holds it now");

        app.ask_quit().expect("nothing to ask");
        assert!(app.prompt.is_none(), "so no box opens");
    }

    #[test]
    fn a_value_set_back_to_the_one_on_the_flash_leaves_nothing_to_ask() {
        let mut app = App::at(daemon_on_sim());
        app.refreshed().expect("a state");
        let was = app.values.get("led.bri").cloned().expect("the value");
        select(&mut app, "led.bri");

        app.begin_edit().expect("the key is editable");
        app.edit_push('9');
        app.commit_edit().expect("the value is written");
        assert_eq!(app.unsaved().len(), 1);

        app.begin_edit().expect("the key is editable");
        app.editing.as_mut().expect("an edit").text = match &was {
            Value::Scalar(number) => number.to_string(),
            other => panic!("{other:?}"),
        };
        app.commit_edit().expect("the value is written back");
        assert!(
            app.unsaved().is_empty(),
            "a change that is no change needs no question"
        );
    }

    #[test]
    fn the_options_open_on_the_paths_their_names_carry() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");

        let rows = app.rows();
        assert_eq!(
            rows,
            vec![
                Row::Group {
                    segment: "map".into(),
                    keys: 1
                },
                Row::Group {
                    segment: "led".into(),
                    keys: 5
                },
                Row::Group {
                    segment: "usb".into(),
                    keys: 1
                },
            ],
            "the top of the menu is the first segment of every key's name, and a \
             segment counts the keys anywhere below it"
        );
    }

    #[test]
    fn a_segment_is_gone_into_and_backed_out_of() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.selected = 1; // led

        app.open_selected().expect("the level opens");
        assert_eq!(app.path, vec!["led".to_string()]);
        assert_eq!(app.selected, 0);
        let rows = app.rows();
        assert_eq!(
            rows.iter()
                .filter(|row| matches!(row, Row::Key(_)))
                .count(),
            4,
            "the keys whose names end here are the level"
        );
        assert_eq!(
            rows.last(),
            Some(&Row::Group {
                segment: "rgb".into(),
                keys: 1
            }),
            "and the segment `led.rgb` keeps its place in the declaration"
        );

        // The deeper segment is the last line of the level.
        app.selected = rows.len() - 1;
        app.open_selected().expect("the deeper level opens");
        assert_eq!(app.path, vec!["led".to_string(), "rgb".to_string()]);
        assert_eq!(app.rows().len(), 1, "one key is left at this level");
        assert_eq!(app.selected_key().map(|at| app.keys[at].name.as_str()), Some("led.rgb.hz"));

        app.go_up().expect("back out one level");
        assert_eq!(app.path, vec!["led".to_string()]);
        assert_eq!(
            app.focused_segment(),
            Some(("rgb".to_string(), 1)),
            "the focus lands on the segment that led in"
        );

        app.go_up().expect("back out to the top");
        assert!(app.path.is_empty());
        assert!(
            !app.rows().iter().any(|row| matches!(row, Row::Key(_))),
            "no key sits at the top of the menu"
        );
        assert!(app.go_up().is_ok(), "backing out of the top does nothing");
    }

    #[test]
    fn a_key_is_edited_rather_than_gone_into() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        app.selected = 1;
        app.open_selected().expect("led opens");
        select(&mut app, "led.mode");

        app.go_in().expect("right on a key");
        assert_eq!(app.path, vec!["led".to_string()], "a key has no level below");
        assert!(app.editing.is_none(), "and right does not edit it either");

        app.open_selected().expect("enter on a key");
        assert!(
            matches!(
                app.editing.as_ref().map(|editing| editing.target),
                Some(Target::Key(_))
            ),
            "enter is what edits a key"
        );
    }

    #[test]
    fn the_screen_names_the_level_the_menu_is_on() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");

        let top = screen(&app);
        assert!(top.contains("options (7)"), "the top is the whole menu");
        assert!(top.contains("▸ led"), "a segment is marked as one");
        assert!(
            !top.contains("LED Mode"),
            "and the keys inside it are not on the top level"
        );
        // The column beside the menu says what the line in focus is for, and
        // the names a value may take are read where the value is written.
        assert!(
            top.contains("How the"),
            "the column says what the segment is for"
        );

        app.selected = 1;
        app.open_selected().expect("led opens");
        let inside = screen(&app);
        assert!(
            inside.contains("options › led"),
            "the title carries the path: {}",
            inside.lines().take(3).collect::<Vec<_>>().join(" / ")
        );
        assert!(inside.contains("LED Mode"));
        assert!(inside.contains("▸ rgb"), "a deeper segment is offered too");
        // The column wraps at eleven characters, so the words are what is asked
        // for rather than a phrase that would be broken across lines.
        assert!(
            inside.contains("Brightness") && inside.contains("strip"),
            "the column follows the line in focus"
        );
    }

    #[test]
    fn an_effect_is_offered_by_the_name_the_schema_gives_it() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        select(&mut app, "led.mode");

        let (_, choices, current) = app.options();
        assert_eq!(current, Some(1), "the simulated strip runs the rainbow");
        let names: Vec<&str> = choices.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Off", "Rainbow", "Solid"],
            "every effect the schema names is offered"
        );
    }

    #[test]
    fn a_key_is_edited_from_the_level_it_sits_on() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        select(&mut app, "led.mode");

        app.begin_edit().expect("the key is editable");
        assert!(
            screen(&app).contains("edit · led.mode"),
            "the box names the key it holds"
        );
        assert!(
            screen(&app).contains("LEDMode"),
            "and the enum its value is drawn from"
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

        let text = |app: &App| app.editing.as_ref().map(|editing| editing.text.clone());
        let opening = text(&app);
        app.edit_step(1);
        assert_ne!(text(&app), opening, "a key steps the value");
        app.edit_step(-1);
        assert_eq!(text(&app), opening, "and steps it back");
    }

    #[test]
    fn a_typed_digit_replaces_the_value_it_opens_on() {
        let mut app = App::at(daemon_on_sim());
        app.refresh().expect("a state");
        select(&mut app, "led.bri");
        app.begin_edit().expect("the key is editable");
        app.edit_push('9');

        assert_eq!(
            app.editing.as_ref().map(|editing| editing.text.as_str()),
            Some("9"),
            "the first digit is the value, not a digit appended to it"
        );
        app.commit_edit().expect("the value is written");
        assert_eq!(app.values.get("led.bri"), Some(&Value::Scalar(9)));
    }

    /// The screen as a terminal holds it: the cells, with what each is drawn in.
    fn buffer_of(app: &App) -> ratatui::buffer::Buffer {
        let backend = ratatui::backend::TestBackend::new(120, 40);
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
            boxed.contains("DOWN (1)") && boxed.contains("B4 (7)"),
            "the names around the one held are in it"
        );
        assert!(
            boxed.contains("→ B1 (4)"),
            "the value being typed is the one marked"
        );

        // The box keeps to the device's column: what a value may be is still
        // read on the right while it is typed.
        // The body takes 68% of the frame's width: nothing of the box may be
        // drawn past it.
        let choices = |line: &str| line.chars().skip(82).collect::<String>();
        for (at, (left, right)) in plain.lines().zip(boxed.lines()).enumerate() {
            assert_eq!(
                choices(left),
                choices(right),
                "line {at} of the choices column was drawn over"
            );
        }
    }
}
