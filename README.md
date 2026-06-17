# Dashboard

A PipeWire crossfade mixer. Dashboard creates two virtual outputs that you can
route your audio to, and mix between them with a single control. For example: 
send chat to one side, and game audio to the other.

![Dashboard](data/screenshots/main-window.png)

## Building with Meson

Requires GTK4, libadwaita, WirePlumber (`wireplumber-0.5`), PipeWire, and a Rust toolchain (edition 2024, rustc ≥1.96).

```
meson setup builddir
meson compile -C builddir
meson install -C builddir
```

`meson setup builddir --buildtype=release` for an optimized build. 

After the install steps, run the app from the install prefix.

## Building with Flatpak

The manifest pulls WirePlumber and builds against the GNOME 50
runtime, with the Rust and LLVM SDK extensions:

```
flatpak install flathub org.gnome.Platform//50 org.gnome.Sdk//50 \
    org.freedesktop.Sdk.Extension.rust-stable \
    org.freedesktop.Sdk.Extension.llvm22
```

The `shared-modules` submodule and a generated `cargo-sources.json` both have 
to be available first:

```
git submodule update --init
./generate-cargo-sources.sh
```

Then build and install:

```
flatpak-builder --user --install --force-clean builddir io.github.arulan.Dashboard.json
flatpak run io.github.arulan.Dashboard
```

Run `generate-cargo-sources.sh` again whenever `Cargo.lock` changes.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
