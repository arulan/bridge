// Copyright (C) 2026 arulan
//
// This file is part of Bridge.
//
// Bridge is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bridge is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Bridge. If not, see <https://www.gnu.org/licenses/>.

mod application;
mod audio;
mod config;
mod dialogs;
mod shortcuts;
mod util;
mod volume;
mod window;

use adw::prelude::*;
use application::{BridgeApplication, RESOURCES_FILE, register_actions};

fn main() -> glib::ExitCode {
    let path = RESOURCES_FILE.expect("RESOURCES_FILE not set; build with meson");
    let resources = gio::Resource::load(path).expect("failed to load resources");
    gio::resources_register(&resources);

    let app = BridgeApplication::new();
    register_actions(&app);
    app.run()
}
