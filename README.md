# Hashman

> [!CAUTION]
> This is WIP software, not recommended for use yet.

Small desktop app (Windows / macOS / Linux) with two buttons:

- **Generate**: select a folder, the tool generates and writes `MANIFEST.sha256` into it.
- **Validate**: select a folder with a `MANIFEST.sha256` file, the tool validates the manifest agains the files on disk.

The `MANIFEST.sha256` uses the standard `hash  relative/path` format.  
Validation and generation may take a long time depending on file size, disk speed and cpu speed.

## Build locally

Prerequisites: [Rust](https://rustup.rs), plus the Tauri system dependencies for your OS (https://tauri.app/start/prerequisites/).

```bash
cargo install tauri-cli --version "^2" --locked
cargo tauri icon app-icon.png     # one-time: generates src-tauri/icons/* from app-icon.png
cargo tauri dev                   # run in dev mode
cargo tauri build                 # installers land in src-tauri/target/release/bundle/
```

Run the core tests (no GUI libraries needed beyond Tauri's normal build deps):

```bash
cd src-tauri && cargo test
```


