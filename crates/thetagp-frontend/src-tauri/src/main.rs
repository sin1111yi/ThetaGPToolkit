//! The window: the device's own commands, as the web side asks them.
//!
//! Every command is a shell over [`device`], which is a shell over the client:
//! the web side names a key and a value, and the device is what decides whether
//! the value holds.

mod device;

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use tauri::State;

use device::{ConnState, Device, Identity, Profiles, Restored, Saved};
use thetagp_client::{Key, Value};

/// The state, whether or not a command that failed left it poisoned: the
/// connection is what it is, and a lost lock is not a lost session.
fn held<'a>(device: &'a State<'_, Mutex<Device>>) -> MutexGuard<'a, Device> {
    match device.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[tauri::command]
fn state(device: State<'_, Mutex<Device>>) -> Result<ConnState, String> {
    held(&device).state()
}

/// Every source a connection can name: the device that answers in the daemon's
/// own process, and the serial ports this machine carries.
#[tauri::command]
fn ports() -> Result<Vec<String>, String> {
    let mut sources = vec![device::SIMULATED.to_string()];
    sources.extend(
        thetagp_client::transport::available_ports().map_err(|error| error.to_string())?,
    );
    Ok(sources)
}

#[tauri::command]
fn connect(device: State<'_, Mutex<Device>>, source: String) -> Result<ConnState, String> {
    held(&device).connect(&source)
}

#[tauri::command]
fn disconnect(device: State<'_, Mutex<Device>>) -> Result<ConnState, String> {
    held(&device).disconnect()
}

#[tauri::command]
fn identity(device: State<'_, Mutex<Device>>) -> Result<Identity, String> {
    held(&device).identity()
}

#[tauri::command]
fn keys(device: State<'_, Mutex<Device>>) -> Result<Vec<Key>, String> {
    held(&device).keys()
}

#[tauri::command]
fn config(device: State<'_, Mutex<Device>>) -> Result<BTreeMap<String, Value>, String> {
    held(&device).config()
}

#[tauri::command]
fn set_key(device: State<'_, Mutex<Device>>, key: String, value: Value) -> Result<Value, String> {
    held(&device).set_key(&key, &value)
}

#[tauri::command]
fn profiles(device: State<'_, Mutex<Device>>) -> Result<Profiles, String> {
    held(&device).profiles()
}

#[tauri::command]
fn select_profile(device: State<'_, Mutex<Device>>, id: u32) -> Result<u32, String> {
    held(&device).select_profile(id)
}

#[tauri::command]
fn snapshot(device: State<'_, Mutex<Device>>) -> Result<String, String> {
    held(&device).snapshot()
}

#[tauri::command]
fn restore(device: State<'_, Mutex<Device>>, snapshot: String) -> Result<Restored, String> {
    held(&device).restore(&snapshot)
}

#[tauri::command]
fn save(device: State<'_, Mutex<Device>>) -> Result<Saved, String> {
    held(&device).save()
}

#[tauri::command]
fn load(device: State<'_, Mutex<Device>>) -> Result<(), String> {
    held(&device).load()
}

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(Device::new()))
        .invoke_handler(tauri::generate_handler![
            state,
            ports,
            connect,
            disconnect,
            identity,
            keys,
            config,
            set_key,
            save,
            load,
            profiles,
            select_profile,
            snapshot,
            restore
        ])
        .run(tauri::generate_context!())
        .expect("the window runs");
}
