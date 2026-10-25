// The window: the device's own commands, called through the client's shell.
//
// Nothing here keeps a list of what a key may be: the shape, the range and the
// value all come from the device that declared them.

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

type Identity = {
  board: string;
  version: string;
  build_date: string;
  build_time: string;
};

type ConnState = { source: string | null };

const app = document.querySelector<HTMLDivElement>("#app")!;
app.innerHTML = `
  <header>
    <h1>ThetaGP Toolkit</h1>
    <span id="state" class="muted">disconnected</span>
  </header>
  <section class="bar">
    <select id="source"></select>
    <button id="ports" class="quiet">ports</button>
    <button id="connect">connect</button>
    <button id="disconnect" class="quiet">disconnect</button>
  </section>
  <section id="identity"></section>
  <section id="panel"></section>
  <p id="error"></p>
`;

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const state = byId("state");
const source = byId<HTMLSelectElement>("source");
const identity = byId("identity");
const panel = byId("panel");
const error = byId("error");

function report(failure: unknown) {
  error.textContent = failure === null || failure === undefined ? "" : String(failure);
}

async function listPorts() {
  const ports = await invoke<string[]>("ports");
  const chosen = source.value;
  source.innerHTML = "";
  for (const name of ports) {
    const option = document.createElement("option");
    option.value = name;
    option.textContent = name;
    source.append(option);
  }
  source.value = ports.includes(chosen) ? chosen : ports[0];
}

async function showConnection() {
  const current = await invoke<ConnState>("state");
  state.textContent = current.source ? `connected · ${current.source}` : "disconnected";
  identity.innerHTML = "";
  panel.innerHTML = "";
  if (!current.source) {
    return;
  }

  const named = await invoke<Identity>("identity");
  identity.innerHTML = `
    <strong>${named.board}</strong>
    <span>${named.version}</span>
    <span class="muted">${named.build_date} ${named.build_time}</span>
  `;

  const keys = await invoke<Key[]>("keys");
  const values = await invoke<Record<string, number | number[]>>("config");
  const rows = keys
    .map((key) => {
      const value = values[key.name];
      const shown = Array.isArray(value) ? value.join(", ") : String(value ?? "—");
      return `<tr>
        <td>${key.name}</td>
        <td class="muted">${key.kind}</td>
        <td class="muted">${key.min}..${key.max}</td>
        <td class="value">${shown}</td>
        <td class="muted">${key.reboot ? "at boot" : ""}</td>
      </tr>`;
    })
    .join("");
  panel.innerHTML = `
    <table>
      <thead><tr><th>key</th><th>shape</th><th>range</th><th>value</th><th></th></tr></thead>
      <tbody>${rows}</tbody>
    </table>
  `;
}

function guard(action: () => Promise<void>) {
  return () => {
    error.textContent = "";
    action().catch(report);
  };
}

byId("ports").addEventListener("click", guard(listPorts));
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

guard(async () => {
  await listPorts();
  await showConnection();
})();
