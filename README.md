# ThetaGPToolkit

The host side of ThetaGP: the protocol client, the device's own tools and the
MCP server, in one Rust workspace.

## What it is

One Cargo workspace, one core, and the consumers that share it: the **frontend**
and the **MCP server** both call `thetagp-client` in the process they run in.
Nothing is reached through a command line, and no consumer is the backend.

- The protocol is `ThetaGP.PB`, pinned by a submodule at a tag: the toolkit
  speaks the same wire the firmware does, at one version on both sides.
- **It does not depend on the firmware**, and the firmware does not depend on
  it: the firmware builds with no toolkit present, and the toolkit never reads a
  firmware checkout or a firmware build directory.
- A value is held to the configuration key the **device itself declares**, so a
  mistyped name or a value out of range is refused where it was typed rather
  than by the device.
- A profile is a body that travels in a stream, and a snapshot is the whole of
  what a device is set to: its configuration and every profile, as one file.

## Features

- The CDC wire: a varint length, the payload, and the payload's byte sum, with
  the receive side that frames it and the numbering that pairs a reply with its
  request.
- The messages of the protocol, generated from the pinned schema, and its
  descriptor set, read for what has no Rust type.
- The device's own commands: the configuration and its keys, the profiles and
  their bodies, the diagnostics and the flash, and a snapshot of the whole of it.
- An MCP server over stdio, so a model can work the device and the protocol.
- A simulated device, so every one of those paths runs with no board on the desk.

## Quick Start

```bash
git submodule update --init   # the protocol schema, pinned to a tag
cargo test
cargo run -p thetagp-mcp -- --simulated   # the server, over a device in-process
```

Needs Rust 1.85+, `protoc` (the schema is compiled into Rust types at build
time) and — to talk to a board — the device's own USB interface.

## Layout

```
crates/
  thetagp-wire/     the wire: framing, the checksum, the half frame's timeout
  thetagp-proto/    the messages, generated from the pinned schema
  thetagp-client/   the exchange, and the device as a model of it:
                    transport, session, keys, config, backup, dynamic, sim
  thetagp-mcp/      the device and the protocol, as tools a model can call
```

`thetagp-cli` is not here: a console is a tool of the MCP server and a panel of
the frontend, and what was protocol in the drafted one lives in
`thetagp-client::dynamic`.

## Dependencies

| Crate | Purpose |
|---|---|
| `ThetaGP.PB` | the protocol schema, a submodule pinned to a tag |
| `prost` / `prost-reflect` | the messages, and the messages read by name |
| `serialport` | the connection to a device |
| `clap` | the server's own arguments |
| `serde` / `serde_json` | the snapshots a file carries, and the MCP wire |

## License

GPL-3.0
