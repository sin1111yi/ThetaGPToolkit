//! A device's configuration and its profiles as one file: the whole of what it
//! is set to, so it can be kept, shared, or put onto another device.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::keys::{Reject, Value};
use crate::session::{Session, SessionError};
use crate::transport::Transport;

/// What a device was set to, as a file carries it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// The board it was taken from, as the device named itself.
    pub board: String,
    /// The firmware it was taken from.
    pub version: String,
    /// Every key the device carried and the value it held.
    pub config: BTreeMap<String, Value>,
    /// Every profile the device carried, in id order.
    pub profiles: Vec<StoredProfile>,
}

/// One profile of a snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredProfile {
    pub id: u32,
    /// The profile's body: the JSON the store keeps.
    pub body: String,
}

/// What a restore wrote, and what the target would not take.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Restored {
    pub values_written: usize,
    pub profiles_written: usize,
    /// Keys the snapshot carries and the device does not declare.
    pub keys_unknown: Vec<String>,
    /// Keys the device declares and the value does not fit.
    pub keys_refused: Vec<String>,
}

impl Snapshot {
    /// Takes a device's configuration and every one of its profiles.
    pub fn take<T: Transport>(session: &mut Session<T>) -> Result<Snapshot, SessionError> {
        let named = session.fw_version()?;
        let config = Config::read(session)?;

        let mut profiles = Vec::new();
        for entry in session.profiles()?.profiles {
            let body = session.profile_get(entry.id)?;
            profiles.push(StoredProfile {
                id: entry.id,
                body: String::from_utf8_lossy(&body).into_owned(),
            });
        }

        Ok(Snapshot {
            board: named.board,
            version: named.version,
            config: config.values().clone(),
            profiles,
        })
    }

    /// Writes the snapshot's values and profiles onto a device.
    ///
    /// A snapshot may come from another board or another firmware, so every
    /// value is held against the keys the target declares: a key the target has
    /// not, and one whose value does not fit, are named in the report rather
    /// than written.
    pub fn restore<T: Transport>(
        &self,
        session: &mut Session<T>,
    ) -> Result<Restored, SessionError> {
        let space = session.key_space()?;
        let mut restored = Restored::default();

        for (name, value) in &self.config {
            match space.check(name, value) {
                Ok(()) => {
                    session.write_value(name, value)?;
                    restored.values_written += 1;
                }
                Err(Reject::UnknownKey(_)) => restored.keys_unknown.push(name.clone()),
                Err(_) => restored.keys_refused.push(name.clone()),
            }
        }

        for profile in &self.profiles {
            session.profile_put(profile.id, profile.body.as_bytes())?;
            restored.profiles_written += 1;
        }

        Ok(restored)
    }

    /// The snapshot as the text a file carries.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// The snapshot a file's text carries.
    pub fn from_json(text: &str) -> Result<Snapshot, serde_json::Error> {
        serde_json::from_str(text)
    }
}
