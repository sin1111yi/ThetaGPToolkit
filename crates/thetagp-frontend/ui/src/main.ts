// The window: the device's own commands, called through the client's shell.
//
// Nothing here keeps a list of what a key may be: the shape, the range and the
// value all come from the device that declared them. A value is written when it
// is changed, and `save` is what writes the store.

import { invoke } from "@tauri-apps/api/core";
import "./style.css";

type Key = {
  name: string;
  domain: string;
  leaf: string;
  kind: string;
  min: number;
  max: number;
  count: number;
  reboot: boolean;
};

type Value = number | number[];

type Identity = {
  board: string;
  version: string;
  build_date: string;
  build_time: string;
};

type ConnState = { source: string | null };
type Saved = { persisted: boolean; dropped_keys: number | null };
type ProfileEntry = { id: number; active: boolean };
type Profiles = { active: number; profiles: ProfileEntry[] };
type Restored = {
  values_written: number;
  profiles_written: number;
  keys_unknown: string[];
  keys_refused: string[];
};

const app = document.querySelector<HTMLDivElement>("#app")!;
app.innerHTML = `
  <header>
    <h1>ThetaGP Toolkit</h1>
    <span id="state" class="muted">disconnected</span>
  </header>
  <section class="bar">
    <select id="source"></select>
    <button id="ports" class="quiet">sources</button>
    <button id="connect">connect</button>
    <button id="disconnect" class="quiet">disconnect</button>
  </section>
  <section id="identity"></section>
  <section class="bar" id="tools" hidden>
    <button id="save">save</button>
    <button id="reload" class="quiet">reload</button>
    <span id="dirty" class="muted"></span>
  </section>
  <section id="panel"></section>
  <section class="bar" id="profiles-bar" hidden>
    <strong>profiles</strong>
    <button id="take-snapshot" class="quiet">take snapshot</button>
    <span id="snapshot-note" class="muted"></span>
  </section>
  <section id="profiles"></section>
  <section id="snapshot-panel" hidden>
    <textarea id="snapshot-text" spellcheck="false"></textarea>
    <button id="restore">restore</button>
  </section>
  <p id="error"></p>
`;

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const state = byId("state");
const source = byId<HTMLSelectElement>("source");
const identity = byId("identity");
const panel = byId("panel");
const error = byId("error");
const tools = byId("tools");
const dirty = byId("dirty");
const profilesBar = byId("profiles-bar");
const profilesBox = byId("profiles");
const snapshotPanel = byId("snapshot-panel");
const snapshotText = byId<HTMLTextAreaElement>("snapshot-text");
const snapshotNote = byId("snapshot-note");

let keys: Key[] = [];
let values: Record<string, Value> = {};
let unsaved = new Set<string>();

function report(failure: unknown) {
  error.textContent = failure === null || failure === undefined ? "" : String(failure);
}

function showUnsaved() {
  dirty.textContent = unsaved.size === 0 ? "" : `${unsaved.size} not saved`;
}

async function listSources() {
  const sources = await invoke<string[]>("ports");
  const chosen = source.value;
  source.innerHTML = "";
  for (const name of sources) {
    const option = document.createElement("option");
    option.value = name;
    option.textContent = name;
    source.append(option);
  }
  source.value = sources.includes(chosen) ? chosen : (sources[0] ?? "");
}

function parseValue(key: Key, raw: string): Value {
  if (key.kind.endsWith("Array")) {
    const parts = raw
      .split(",")
      .map((part) => part.trim())
      .filter((part) => part.length > 0)
      .map((part) => Number(part));
    if (parts.some((part) => !Number.isFinite(part))) {
      throw new Error("a run of numbers, separated by commas");
    }
    return parts;
  }
  const value = Number(raw);
  if (!Number.isFinite(value)) {
    throw new Error("a number");
  }
  return value;
}

function renderKey(key: Key): HTMLTableRowElement {
  const row = document.createElement("tr");
  const value = values[key.name];
  const shown = Array.isArray(value) ? value.join(", ") : String(value ?? "");

  row.innerHTML = `
    <td title="${key.name}">${key.name}</td>
    <td class="muted">${key.kind}${key.kind.endsWith("Array") ? ` · ${key.count}` : ""}</td>
    <td class="muted">${key.min}..${key.max}</td>
    <td class="value"><input /></td>
    <td class="muted">${key.reboot ? "at boot" : ""}</td>
    <td class="note"></td>
  `;

  const input = row.querySelector("input") as HTMLInputElement;
  const note = row.querySelector(".note") as HTMLElement;
  input.type = key.kind.endsWith("Array") ? "text" : "number";
  if (!key.kind.endsWith("Array")) {
    input.min = String(key.min);
    input.max = String(key.max);
  }
  input.value = shown;

  const write = async () => {
    note.textContent = "";
    error.textContent = "";
    try {
      const next = parseValue(key, input.value);
      await invoke<Value>("set_key", { key: key.name, value: next });
      values[key.name] = next;
      unsaved.add(key.name);
      note.textContent = "written";
      showUnsaved();
    } catch (failure) {
      note.textContent = String(failure);
    }
  };

  input.addEventListener("change", write);
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      input.blur();
    }
  });

  return row;
}

function renderKeys() {
  const rows = keys.map(renderKey);
  const table = document.createElement("table");
  table.innerHTML = `
    <thead><tr><th>key</th><th>shape</th><th>range</th><th>value</th><th></th><th></th></tr></thead>
  `;
  const body = document.createElement("tbody");
  for (const row of rows) {
    body.append(row);
  }
  table.append(body);
  panel.replaceChildren(table);
}

async function refreshProfiles() {
  const listed = await invoke<Profiles>("profiles");
  profilesBox.replaceChildren();
  for (const entry of listed.profiles) {
    const row = document.createElement("div");
    row.className = "profile";
    const name = document.createElement("span");
    name.textContent = `#${entry.id}${entry.active ? " · active" : ""}`;
    row.append(name);
    if (!entry.active) {
      const button = document.createElement("button");
      button.className = "quiet";
      button.textContent = "activate";
      button.addEventListener(
        "click",
        guard(async () => {
          await invoke<number>("select_profile", { id: entry.id });
          await refreshProfiles();
          snapshotNote.textContent = `#${entry.id} is the profile the device runs from`;
        }),
      );
      row.append(button);
    }
    profilesBox.append(row);
  }
  if (listed.profiles.length === 0) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = "this device carries no user profiles";
    profilesBox.append(empty);
  }
  profilesBar.hidden = false;
}

async function takeSnapshot() {
  snapshotText.value = await invoke<string>("snapshot");
  snapshotPanel.hidden = false;
  snapshotNote.textContent = "the snapshot is in the box below";
}

async function restoreSnapshot() {
  const report = await invoke<Restored>("restore", { snapshot: snapshotText.value });
  const refused = [...report.keys_unknown, ...report.keys_refused];
  snapshotNote.textContent =
    `wrote ${report.values_written} values and ${report.profiles_written} profiles` +
    (refused.length === 0 ? "" : `; not taken: ${refused.join(", ")}`);
  await refreshProfiles();
  values = await invoke<Record<string, Value>>("config");
  unsaved.clear();
  showUnsaved();
  renderKeys();
}

async function showConnection() {
  const current = await invoke<ConnState>("state");
  state.textContent = current.source ? `connected · ${current.source}` : "disconnected";
  identity.innerHTML = "";
  panel.innerHTML = "";
  profilesBox.replaceChildren();
  tools.hidden = true;
  profilesBar.hidden = true;
  snapshotPanel.hidden = true;
  unsaved.clear();
  showUnsaved();
  if (!current.source) {
    return;
  }

  const named = await invoke<Identity>("identity");
  identity.innerHTML = `
    <strong>${named.board}</strong>
    <span>${named.version}</span>
    <span class="muted">${named.build_date} ${named.build_time}</span>
  `;

  keys = await invoke<Key[]>("keys");
  values = await invoke<Record<string, Value>>("config");
  renderKeys();
  tools.hidden = false;
  await refreshProfiles();
}

async function saveConfiguration() {
  const saved = await invoke<Saved>("save");
  unsaved.clear();
  showUnsaved();
  const dropped = saved.dropped_keys ?? 0;
  error.textContent = saved.persisted
    ? dropped === 0
      ? "saved to the device"
      : `saved; the device would not keep ${dropped} of the stored values`
    : "the device did not write the configuration";
}

async function reloadConfiguration() {
  await invoke("load");
  values = await invoke<Record<string, Value>>("config");
  unsaved.clear();
  showUnsaved();
  renderKeys();
}

function guard(action: () => Promise<void>) {
  return () => {
    error.textContent = "";
    action().catch(report);
  };
}

byId("ports").addEventListener("click", guard(listSources));
byId("connect").addEventListener(
  "click",
  guard(async () => {
    await invoke("connect", { source: source.value });
    await showConnection();
  }),
);
byId("disconnect").addEventListener(
  "click",
  guard(async () => {
    await invoke("disconnect");
    await showConnection();
  }),
);
byId("save").addEventListener("click", guard(saveConfiguration));
byId("reload").addEventListener("click", guard(reloadConfiguration));
byId("take-snapshot").addEventListener("click", guard(takeSnapshot));
byId("restore").addEventListener("click", guard(restoreSnapshot));

guard(async () => {
  await listSources();
  await showConnection();
})();
