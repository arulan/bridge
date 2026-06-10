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

mod application;
mod audio;
mod config;
mod dialogs;
mod util;
mod window;
mod wp;

use adw::prelude::*;
use application::{register_actions, DashboardApplication};

fn main() -> glib::ExitCode {
    let app = DashboardApplication::new();
    register_actions(&app);
    app.run()
}
