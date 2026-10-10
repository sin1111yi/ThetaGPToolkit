//! A device's configuration: every key it declares and the value each holds.
//!
//! The values are read one arm at a time -- `config.get_key` per key -- and a
//! write is held against the key space first, so the device is not asked for a
//! value it has already refused.

use std::collections::BTreeMap;

use crate::keys::{KeySpace, Reject, Value};
use crate::session::{Session, SessionError};
use crate::transport::Transport;

/// The configuration a device is running.
#[derive(Clone, Debug)]
pub struct Config {
    space: KeySpace,
    values: BTreeMap<String, Value>,
}

impl Config {
    /// A configuration over a space, with no value set yet.
    pub fn new(space: KeySpace) -> Self {
        Config {
            space,
            values: BTreeMap::new(),
        }
    }

    /// The keys the device declares.
    pub fn space(&self) -> &KeySpace {
        &self.space
    }

    /// Every value, by key name.
    pub fn values(&self) -> &BTreeMap<String, Value> {
        &self.values
    }

    /// The value the key holds, when one has been read or set.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    /// Sets a value, refused when the key does not carry one like it.
    pub fn set(&mut self, name: &str, value: Value) -> Result<(), Reject> {
        self.space.check(name, &value)?;
        self.values.insert(name.to_string(), value);
        Ok(())
    }

    /// Reads a device's whole configuration: the keys it declares, then one
    /// value per key.
    pub fn read<T: Transport>(session: &mut Session<T>) -> Result<Config, SessionError> {
        let space = session.key_space()?;
        let mut values = BTreeMap::new();
        for key in space.keys() {
            let answer = session.get_key(&key.name)?;
            values.insert(key.name.clone(), Value::from_answer(&answer));
        }
        Ok(Config { space, values })
    }

    /// Writes every value that differs from `was`, and returns how many were
    /// written.
    pub fn apply<T: Transport>(
        &self,
        session: &mut Session<T>,
        was: &Config,
    ) -> Result<Vec<String>, SessionError> {
        let mut written = Vec::new();
        for (name, value) in &self.values {
            if was.values.get(name) == Some(value) {
                continue;
            }
            session.write_value(name, value)?;
            written.push(name.clone());
        }
        Ok(written)
    }

    /// The names of the keys whose value differs from `other`.
    pub fn diff(&self, other: &Config) -> Vec<String> {
        let mut changed = Vec::new();
        for (name, value) in &self.values {
            if other.values.get(name) != Some(value) {
                changed.push(name.clone());
            }
        }
        for name in other.values.keys() {
            if !self.values.contains_key(name) {
                changed.push(name.clone());
            }
        }
        changed
    }
}
