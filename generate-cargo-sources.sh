#!/usr/bin/env bash
#
# Regenerate cargo-sources.json from Cargo.lock for the flatpak build

set -eu

cd "$(dirname "$0")"

tools_dir=".cargo-sources-tools"
tools_repo="https://github.com/flatpak/flatpak-builder-tools.git"
generator="$tools_dir/cargo/flatpak-cargo-generator.py"

if [ -d "$tools_dir/.git" ]; then
    git -C "$tools_dir" pull --ff-only --quiet
else
    git clone --depth 1 --quiet "$tools_repo" "$tools_dir"
fi

python3 "$generator" Cargo.lock -o cargo-sources.json

echo "cargo-sources.json generated"
