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

let keys: Key[] = [];
let values: Record<string, Value> = {};
let pending = new Set<string>();

function report(failure: unknown) {
  error.textContent = failure === null || failure === undefined ? "" : String(failure);
}

function showDirty() {
  dirty.textContent = pending.size === 0 ? "" : `${pending.size} not saved`;
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
  source.value = sources.includes(chosen) ? chosen : sources[0] ?? "";
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
      pending.add(key.name);
      note.textContent = "written";
      showDirty();
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

async function showConnection() {
  const current = await invoke<ConnState>("state");
  state.textContent = current.source ? `connected · ${current.source}` : "disconnected";
  identity.innerHTML = "";
  panel.innerHTML = "";
  tools.hidden = true;
  pending.clear();
  showDirty();
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
}

async function saveConfiguration() {
  const saved = await invoke<Saved>("save");
  pending.clear();
  showDirty();
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
  pending.clear();
  showDirty();
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

guard(async () => {
  await listSources();
  await showConnection();
})();
