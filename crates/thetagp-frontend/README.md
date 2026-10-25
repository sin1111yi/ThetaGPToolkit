# thetagp-frontend

The window: `src-tauri/` is a Tauri v2 app whose commands are shells over
`thetagp-client`, and `ui/` is the web side it shows.

## Build

```bash
cd crates/thetagp-frontend/ui
bun install
bun run build        # writes ui/dist, which the app embeds at compile time
cd ../..
cargo build -p thetagp-frontend
```

The app opens a window, so it needs a display: run it from a desktop session
(`cargo run -p thetagp-frontend`) rather than from a headless shell.

`simulated` is one of the connection sources, so the whole window works with no
board attached.
