#!/usr/bin/env bash
# Fails if the version in Cargo.toml, Cargo.lock, the AppStream metainfo and (optionally)
# the git tag disagree.   Usage: check-version.sh [v1.2.3]
set -euo pipefail
cd "$(dirname "$0")/../.."

cargo_ver="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n1)"
lock_ver="$(grep -A1 '^name = "hashman"$' Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p')"
meta_ver="$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' packaging/linux/io.github.Artzert2215.Hashman.metainfo.xml | head -n1)"
tag_ver="${1:-}"; tag_ver="${tag_ver#v}"

echo "Cargo.toml=$cargo_ver Cargo.lock=$lock_ver metainfo=$meta_ver tag=${tag_ver:-<none>}"
fail=0
[[ "$lock_ver" == "$cargo_ver" ]] || { echo "Cargo.lock is out of date (run: cargo update -p hashman)"; fail=1; }
[[ "$meta_ver" == "$cargo_ver" ]] || { echo "metainfo <release> does not match Cargo.toml"; fail=1; }
[[ -z "$tag_ver" || "$tag_ver" == "$cargo_ver" ]] || { echo "git tag does not match Cargo.toml"; fail=1; }
exit "$fail"
