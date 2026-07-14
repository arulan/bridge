# Bridge

A PipeWire crossfade mixer. Bridge creates two virtual outputs that you can
route your audio to, and mix between them with a single control. For example: 
send chat to one side, and game audio to the other.

![Bridge](data/screenshots/main-window.png)

## Building with Meson

Requires GTK4, libadwaita, PipeWire, and a Rust toolchain (edition 2024, rustc ≥1.96).

```
meson setup builddir
meson compile -C builddir
meson install -C builddir
```


After the install steps, run the app from the install prefix.

## Building with Flatpak

The manifest builds against the GNOME 50 runtime, with the Rust and
LLVM SDK extensions:

```
flatpak install flathub org.gnome.Platform//50 org.gnome.Sdk//50 \
    org.freedesktop.Sdk.Extension.rust-stable \
    org.freedesktop.Sdk.Extension.llvm22
```

A generated `cargo-sources.json` has to be available first:

```
./generate-cargo-sources.sh
```

Then build and install:

```
flatpak-builder --user --install --force-clean builddir io.github.arulan.Bridge.json
flatpak run io.github.arulan.Bridge
```

Run `generate-cargo-sources.sh` again whenever `Cargo.lock` changes.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
