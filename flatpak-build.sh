#!/usr/bin/env bash
#
# defaults to the dev manifest; pass 'prod' for the stable app instead
# Pass 'run' to launch after building

set -eu

cd "$(dirname "$0")"

manifest=io.github.arulan.Bridge-dev.json
app_id=io.github.arulan.Bridge.Devel
run=

for arg in "$@"; do
    case "$arg" in
        prod)
            manifest=io.github.arulan.Bridge.json
            app_id=io.github.arulan.Bridge
            ;;
        run)
            run=yes
            ;;
        *)
            echo "usage: $0 [prod] [run]" >&2
            exit 1
            ;;
    esac
done

flatpak-builder --user --install --force-clean builddir "$manifest"

if [ -n "$run" ]; then
    flatpak run "$app_id"
fi
