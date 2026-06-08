// Copyright (C) 2026 arulan
//
// This file is part of Dashboard.
//
// Dashboard is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Dashboard is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Dashboard. If not, see <https://www.gnu.org/licenses/>.

use std::process::Command;

fn main() {
    // GSETTINGS_SCHEMA_DIR=$PWD/data cargo run
    let status = Command::new("glib-compile-schemas")
        .arg("data")
        .status()
        .expect("failed to run glib-compile-schemas");
    assert!(status.success(), "glib-compile-schemas failed");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=data/io.github.arulan.Dashboard.gschema.xml");
}
