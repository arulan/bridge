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

// Not used yet. For the Setup...
#![allow(dead_code)]

use gio::prelude::*;

const SCHEMA_ID: &str = "io.github.arulan.Dashboard";


#[derive(Clone, Debug)]
pub struct SinkConfig {
    pub aux_channels:  u32,
    pub main_channels: u32,
    pub aux_position:  String,
    pub main_position: String,
    pub aux_hw_name:      String,
    pub main_hw_name:     String,
    pub aux_display_name:  String,
    pub main_display_name: String,
}

fn settings() -> gio::Settings {
    gio::Settings::new(SCHEMA_ID)
}

pub fn load() -> SinkConfig {
    let s = settings();
    SinkConfig {
        aux_channels:  s.int("aux-channels") as u32,
        main_channels: s.int("main-channels") as u32,
        aux_position:  s.string("aux-position").into(),
        main_position: s.string("main-position").into(),
        aux_hw_name:      s.string("aux-hw-name").into(),
        main_hw_name:     s.string("main-hw-name").into(),
        aux_display_name:  s.string("aux-display-name").into(),
        main_display_name: s.string("main-display-name").into(),
    }
}


pub fn store(cfg: &SinkConfig) {
    let s = settings();
    let _ = s.set_int("aux-channels", cfg.aux_channels as i32);
    let _ = s.set_int("main-channels", cfg.main_channels as i32);
    let _ = s.set_string("aux-position", &cfg.aux_position);
    let _ = s.set_string("main-position", &cfg.main_position);
    let _ = s.set_string("aux-hw-name", &cfg.aux_hw_name);
    let _ = s.set_string("main-hw-name", &cfg.main_hw_name);
    let _ = s.set_string("aux-display-name", &cfg.aux_display_name);
    let _ = s.set_string("main-display-name", &cfg.main_display_name);
}
