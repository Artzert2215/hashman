#!/usr/bin/env bash
# Generates packaging/flatpak/cargo-sources.json from Cargo.lock so flatpak-builder
# can build offline. The generated file is NOT committed (see .gitignore).
set -euo pipefail
cd "$(dirname "$0")/../.."

# TODO: pin GENERATOR_REF to a commit SHA of flatpak/flatpak-builder-tools for reproducibility.
GENERATOR_REF="${GENERATOR_REF:-master}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

python3 -m venv "$tmp/venv"
"$tmp/venv/bin/pip" install --quiet aiohttp tomlkit
curl -fsSL "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/${GENERATOR_REF}/cargo/flatpak-cargo-generator.py" \
  -o "$tmp/flatpak-cargo-generator.py"
"$tmp/venv/bin/python" "$tmp/flatpak-cargo-generator.py" Cargo.lock -o packaging/flatpak/cargo-sources.json
echo "wrote packaging/flatpak/cargo-sources.json"
