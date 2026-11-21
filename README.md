# Hashman

> [!CAUTION]
> This is WIP software, not recommended for use yet.

Small desktop app (Windows / macOS / Linux) with two buttons:

- **Generate**: select a folder, the tool generates and writes `MANIFEST.sha256` into it.
- **Validate**: select a folder with a `MANIFEST.sha256` file, the tool validates the manifest agains the files on disk.

The `MANIFEST.sha256` uses the standard `hash  relative/path` format.  
Validation and generation may take a long time depending on file size, disk speed and cpu speed.

## Build locally

### Core (headless)

```bash
cargo build        # builds the hashing lib + CLI-less stub
cargo test         # runs the hashing unit tests + the headless UI render test
```

No display or X11/Wayland libraries are needed for this.

### Windowed app (egui/eframe)

Requires egui's native backend deps for your OS:

- **Linux**: X11/Wayland dev libs
  `libgtk-3-dev libxkbcommon-dev librsvg2-dev libwebp-dev`.
  ```bash
  cargo build --features gui --release
  ./target/release/hashman
  ```
- **macOS**: just Xcode (Cocoa is linked by eframe).
  ```bash
  cargo build --features gui --release
  ./target/release/hashman
  ```
- **Windows**: MSVC toolchain (default on GitHub Actions / MSVC).
  ```bash
  cargo build --features gui --release
  target\x86_64-pc-windows-msvc\release\hashman.exe
  ```

## Running the tests

```bash
cargo test          # hashing + headless UI render test (egui_kittest)
cargo test --lib    # lib only
cargo test --features gui  # also type-checks main.rs
```

## Release builds

| Platform | Artifact |
|----------|----------|
| macOS    | universal `.dmg` |
| Linux    | `hashman-linux.tar.gz` (binary + bundled shared libs) |
| Windows  | `hashman-windows.zip` (`hashman.exe`) |
