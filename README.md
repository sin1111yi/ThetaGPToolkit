# ThetaGPToolkit

The host side of ThetaGP: the protocol client, the device's own tools and the
MCP server, in one Rust workspace.

## What it is

One Cargo workspace, one core, and one process that owns the device: the
**daemon**. The **terminal window** and the **MCP server** ask it over a local
socket, so both can be open at once and a device that re-enumerates is picked up
again under them.

- The protocol is `ThetaGP.PB`, a submodule that follows the schema's `main`:
  the toolkit speaks the same wire the firmware does, both sides carrying the
  one commit the branch is on.
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
- The messages of the protocol, generated from the schema it carries, and its
  descriptor set, read for what has no Rust type.
- The device's own commands: the configuration and its keys, the profiles and
  their bodies, the diagnostics, the firmware's log and the flash, and a
  snapshot of the whole of it.
- A daemon that holds the device, so a window and a model share one board, and a
  socket and a token that only its owner can read in front of it.
- An MCP server over stdio, so a model can work the device and the protocol.
- A terminal window over the daemon: the sources, the board's identity, the
  configuration as a menu grouped by the path each key's name carries, the
  button remap table one input at a time, the profiles, and a snapshot taken and
  put back. A value is edited in a box over the device, with the names it may
  take scrolling under it; a hue and a saturation are edited on the band they
  belong to, drawn from the rest of the colour the profile carries. `--frame`
  renders one screen as text, for a log or a machine with no display.
- A live view of the input itself, on `i` or `--input`: the keys the host reads
  as held and every axis the device carries, with where it stands along its
  range. It is the state the host's own driver holds -- the bytes a game binds
  to -- read through the kernel, so nothing is asked of the firmware and no
  command channel is shared with it.
- Beside the device: a column saying what the line in focus is for, and under it
  the **firmware's own log**, read over the protocol -- the lines the device
  kept in its ring, with the level each was written at.
- A question before a profile goes and before the screen does, so a change that
  is only on the device is not lost with the window.
- A simulated device, so every one of those paths runs with no board on the desk.

## Quick Start

```bash
git submodule update --init      # the protocol schema
git submodule update --remote    # to the newest schema on its main
cargo test
cargo run -p thetagp-daemon              # the device's host; Ctrl-C to stop it
cargo run -p thetagp-mcp -- --simulated  # the tools, over a device in-process
```

```bash
cargo run -p thetagp-tui                          # a board, or `simulated`
cargo run -p thetagp-tui -- --frame               # one screen, as text
cargo run -p thetagp-tui -- --frame --source /dev/ttyACM0
```

Keys: `tab` between the panes; `up`/`down` move, `left`/`right` go out of and
into a level of the options menu, `enter` opens a segment, edits a value or a
remap slot, or activates a profile; `w` save, `l` load, `t` snapshot, `r`
restore, `d` disconnect, `q` quit -- which asks first when a change has not been
saved. In a box, `up`/`down` walk the values a key takes, `left`/`right` walk a
band, and the first digit typed replaces what was there.

A client that finds no daemon starts one beside itself, so the daemon is not
something to run by hand unless its log is wanted.

Needs Rust 1.85+, `protoc` (the schema is compiled into Rust types at build
time) and — to talk to a board — the device's own USB interface: `/dev/ttyACM*`
for the daemon on Linux, which a source checkout opens as the user who runs it.

## Layout

```
crates/
  thetagp-wire/     the wire: framing, the checksum, the half frame's timeout
  thetagp-proto/    the messages, generated from the schema
  thetagp-client/   the exchange, and the device as a model of it:
                    transport, session, keys, config, backup, dynamic, sim
  thetagp-tools/    the tools a caller asks the device for, and the one place
                    each is executed
  thetagp-ipc/      the local link: a JSON-RPC message per line, guarded by a
                    token the daemon records beside its port
  thetagp-daemon/   the one process that owns the device; it answers the tools
                    and the window's typed calls over that link
  thetagp-mcp/      the same tools, as the JSON-RPC a model host speaks over
                    stdio: it passes each call to the daemon
  thetagp-tui/      the window, in a terminal: the daemon's device as an
                    options menu, the remap table, the profiles and the
                    firmware's log, with `--frame` for no display
```

A device is one port, so one process opens it: the daemon. Everything else --
the window, the model's tools -- asks the daemon rather than the device. Start
it by hand to read its log; the clients start one beside themselves when none
is running.

`thetagp-cli` is not here: a console is a tool of the MCP server and a panel of
the frontend, and what was protocol in the drafted one lives in
`thetagp-client::dynamic`.

## Dependencies

| Crate | Purpose |
|---|---|
| `ThetaGP.PB` | the protocol schema, a submodule following its `main` |
| `prost` / `prost-reflect` | the messages, and the messages read by name |
| `serialport` | the connection to a device |
| `clap` | the server's own arguments |
| `serde` / `serde_json` | the snapshots a file carries, and the MCP wire |

## License

GPL-3.0
