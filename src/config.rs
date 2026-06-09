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
    let aux  = s.child("aux");
    let main = s.child("main");
    SinkConfig {
        aux_channels:  aux.int("channels") as u32,
        main_channels: main.int("channels") as u32,
        aux_position:  aux.string("position").into(),
        main_position: main.string("position").into(),
        aux_hw_name:      aux.string("hw-name").into(),
        main_hw_name:     main.string("hw-name").into(),
        aux_display_name:  aux.string("display-name").into(),
        main_display_name: main.string("display-name").into(),
    }
}


pub fn store(cfg: &SinkConfig) {
    let s = settings();
    store_sink(&s.child("aux"), cfg.aux_channels, &cfg.aux_position, &cfg.aux_hw_name, &cfg.aux_display_name);
    store_sink(&s.child("main"), cfg.main_channels, &cfg.main_position, &cfg.main_hw_name, &cfg.main_display_name);
}

fn store_sink(s: &gio::Settings, channels: u32, position: &str, hw_name: &str, display_name: &str) {
    let _ = s.set_int("channels", channels as i32);
    let _ = s.set_string("position", position);
    let _ = s.set_string("hw-name", hw_name);
    let _ = s.set_string("display-name", display_name);
}
