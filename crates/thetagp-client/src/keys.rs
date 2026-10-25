//! The key space: what a device says its configuration is made of.
//!
//! It is the device's own declaration, read from `config.list_keys`, so nothing
//! here is written from a list kept beside the firmware: a key the firmware adds
//! appears, and one it drops goes away. A value is held against it **before** it
//! travels, so a mistyped name or a value out of range is refused where it was
//! typed rather than by the device.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::proto;

/// A configuration value: one number, or a run of them in element order.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Scalar(i64),
    Array(Vec<i32>),
}

impl Value {
    /// The value a `config.get_key` answer carries: one number for a scalar key
    /// and a run for an array key, in element order.
    pub fn from_answer(answer: &proto::ConfigGetKeyOk) -> Value {
        match &answer.value {
            Some(proto::config_get_key_ok::Value::Scalar(number)) => Value::Scalar(*number),
            Some(proto::config_get_key_ok::Value::Array(array)) => {
                Value::Array(array.elements.clone())
            }
            None => Value::Scalar(0),
        }
    }
}

/// What a key's value is made of.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum KeyType {
    U8,
    U16,
    I16,
    U8Array,
}

impl KeyType {
    /// The type the wire's value names, or `None` for one this build does not
    /// know -- a schema newer than the client.
    pub fn from_wire(value: i32) -> Option<Self> {
        Some(match value {
            0 => KeyType::U8,
            1 => KeyType::U16,
            2 => KeyType::I16,
            3 => KeyType::U8Array,
            _ => return None,
        })
    }

    /// The name the type is reported by.
    pub fn name(self) -> &'static str {
        match self {
            KeyType::U8 => "u8",
            KeyType::U16 => "u16",
            KeyType::I16 => "i16",
            KeyType::U8Array => "u8 array",
        }
    }

    /// Whether a value of this type is a run of numbers.
    pub fn is_array(self) -> bool {
        matches!(self, KeyType::U8Array)
    }
}

/// One key, as the device declares it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Key {
    /// The whole name, as the wire spells it: `led.rgb.hz`.
    pub name: String,
    /// The name up to the last dot: `led.rgb`.
    pub domain: String,
    /// The name after the last dot: `hz`.
    pub leaf: String,
    pub kind: KeyType,
    /// The lowest value the key holds, one element at a time for an array.
    pub min: i32,
    /// The highest value the key holds.
    pub max: i32,
    /// The number of elements an array key holds; zero when it is not fixed.
    pub count: u32,
    /// Whether a change to the key takes effect at the next boot.
    pub reboot: bool,
    /// The schema enum the value is drawn from, when its numbers stand for
    /// names: `SOCDMode` for a key whose values are that enum's. `None` for a
    /// key whose value is a plain number.
    pub value_enum: Option<String>,
}

/// Every key a device carries, in the order it declared them.
#[derive(Clone, Default, Debug)]
pub struct KeySpace {
    keys: Vec<Key>,
    by_name: HashMap<String, usize>,
}

impl KeySpace {
    /// The space a device declared.
    pub fn from_entries(entries: &[proto::ConfigKeyEntry]) -> Self {
        let mut keys = Vec::with_capacity(entries.len());
        let mut by_name = HashMap::with_capacity(entries.len());
        for entry in entries {
            let (domain, leaf) = match entry.key.rsplit_once('.') {
                Some((domain, leaf)) => (domain.to_string(), leaf.to_string()),
                None => (String::new(), entry.key.clone()),
            };
            by_name.insert(entry.key.clone(), keys.len());
            keys.push(Key {
                name: entry.key.clone(),
                domain,
                leaf,
                kind: KeyType::from_wire(entry.r#type).unwrap_or(KeyType::U8),
                min: entry.min,
                max: entry.max,
                count: entry.count,
                reboot: entry.reboot,
                value_enum: (!entry.value_enum.is_empty())
                    .then(|| entry.value_enum.clone()),
            });
        }
        KeySpace { keys, by_name }
    }

    /// Every key, in declaration order.
    pub fn keys(&self) -> &[Key] {
        &self.keys
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The key of that name, when the device carries one.
    pub fn get(&self, name: &str) -> Option<&Key> {
        self.by_name.get(name).map(|at| &self.keys[*at])
    }

    /// The keys of one domain, in declaration order.
    pub fn domain(&self, domain: &str) -> Vec<&Key> {
        self.keys
            .iter()
            .filter(|key| key.domain == domain)
            .collect()
    }

    /// Every domain, in the order its first key appears.
    pub fn domains(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        for key in &self.keys {
            if !seen.contains(&key.domain.as_str()) {
                seen.push(key.domain.as_str());
            }
        }
        seen
    }

    /// Whether `value` is one `name` may hold.
    pub fn check(&self, name: &str, value: &Value) -> Result<(), Reject> {
        let key = self
            .get(name)
            .ok_or_else(|| Reject::UnknownKey(name.to_string()))?;

        match (key.kind.is_array(), value) {
            (false, Value::Scalar(number)) => {
                if *number < i64::from(key.min) || *number > i64::from(key.max) {
                    return Err(Reject::OutOfRange {
                        key: name.to_string(),
                        min: key.min,
                        max: key.max,
                        got: *number,
                    });
                }
                Ok(())
            }
            (true, Value::Array(elements)) => {
                if key.count != 0 && elements.len() != key.count as usize {
                    return Err(Reject::ElementCount {
                        key: name.to_string(),
                        expected: key.count,
                        got: elements.len(),
                    });
                }
                for (at, element) in elements.iter().enumerate() {
                    if *element < key.min || *element > key.max {
                        return Err(Reject::ElementOutOfRange {
                            key: name.to_string(),
                            at,
                            min: key.min,
                            max: key.max,
                            got: *element,
                        });
                    }
                }
                Ok(())
            }
            (false, Value::Array(_)) => Err(Reject::NotAScalar(name.to_string())),
            (true, Value::Scalar(_)) => Err(Reject::NotAnArray(name.to_string())),
        }
    }
}

/// Why a value is not one a key may hold.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Reject {
    /// The device declares no key of that name.
    UnknownKey(String),
    /// The key holds one number and a run was given.
    NotAScalar(String),
    /// The key holds a run of numbers and one was given.
    NotAnArray(String),
    /// The value falls outside the range the key declares.
    OutOfRange {
        key: String,
        min: i32,
        max: i32,
        got: i64,
    },
    /// A run of the wrong number of elements.
    ElementCount {
        key: String,
        expected: u32,
        got: usize,
    },
    /// One element falls outside the range the key declares.
    ElementOutOfRange {
        key: String,
        at: usize,
        min: i32,
        max: i32,
        got: i32,
    },
}

impl std::fmt::Display for Reject {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reject::UnknownKey(key) => write!(formatter, "the device carries no key named {key}"),
            Reject::NotAScalar(key) => write!(formatter, "{key} holds one number, not a run"),
            Reject::NotAnArray(key) => write!(formatter, "{key} holds a run, not one number"),
            Reject::OutOfRange { key, min, max, got } => {
                write!(formatter, "{key} holds {min}..{max}, and {got} is outside")
            }
            Reject::ElementCount { key, expected, got } => write!(
                formatter,
                "{key} holds {expected} elements, and {got} were given"
            ),
            Reject::ElementOutOfRange {
                key,
                at,
                min,
                max,
                got,
            } => write!(
                formatter,
                "{key} element {at} holds {min}..{max}, and {got} is outside"
            ),
        }
    }
}

impl std::error::Error for Reject {}
