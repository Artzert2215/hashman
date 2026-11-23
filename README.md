# Hashman

> [!CAUTION]
> This software has not been tested on macOS or Windows.

Small desktop app (Windows / macOS / Linux) with two buttons:

- **Generate**: select a folder, the tool generates and writes `MANIFEST.sha256` into it.
- **Validate**: select a folder with a `MANIFEST.sha256` file, the tool validates the manifest against the files on disk.

The `MANIFEST.sha256` uses the standard `hash  relative/path` format, meaning `sha256sum -c MANIFEST.sha256` works too.
Validation and generation may take a long time depending on file size, disk speed and cpu speed.

## Build locally

### Windowed app (egui/eframe)

```bash
cargo build --release
```

This requires egui's deps for your OS:

- **Linux**: X11/Wayland dev libraries.
- **macOS**: Xcode command line tools.
- **Windows**: MSVC toolchain.

## Running tests

```bash
cargo test --no-default-features
cargo test # also runs the egui_kittest headless UI test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

## Release builds

| Platform | Artifact |
|----------|----------|
| macOS    | universal `Hashman.dmg` |
| Linux    | `Hashman-x86_64.AppImage` and `Hashman.flatpak` |
| Windows  | `Hashman.msi` and `Hashman-Setup.exe` |

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-Apache), at your option.
