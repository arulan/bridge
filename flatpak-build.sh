#!/usr/bin/env bash
#
# builds and installs the dev flatpak manifest. Pass 'run' to also launch afterwards

set -eu

cd "$(dirname "$0")"

flatpak-builder --user --install --force-clean builddir io.github.arulan.Dashboard-dev.json

if [ "${1:-}" = "run" ]; then
    flatpak run io.github.arulan.Dashboard
fi
