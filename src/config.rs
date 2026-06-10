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

use gio::prelude::*;

use crate::audio::hw_sink::HwSink;

const SCHEMA_ID: &str = "io.github.arulan.Dashboard";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Aux,
    Main,
}

#[derive(Clone, Debug, Default)]
pub struct SinkDef {
    pub channels:     u32,
    pub position:     String,
    pub hw_name:      String,
    pub display_name: String,
}

#[derive(Clone, Debug)]
pub struct SinkConfig {
    pub aux:  SinkDef,
    pub main: SinkDef,
}

impl SinkConfig {
    pub fn side(&self, side: Side) -> &SinkDef {
        match side {
            Side::Aux  => &self.aux,
            Side::Main => &self.main,
        }
    }

    pub fn side_mut(&mut self, side: Side) -> &mut SinkDef {
        match side {
            Side::Aux  => &mut self.aux,
            Side::Main => &mut self.main,
        }
    }
}

impl From<HwSink> for SinkDef {
    fn from(sink: HwSink) -> Self {
        SinkDef {
            channels:     sink.channels,
            position:     sink.position,
            hw_name:      sink.name,
            display_name: sink.display_name,
        }
    }
}

fn settings() -> gio::Settings {
    gio::Settings::new(SCHEMA_ID)
}

// true after first-run setup
pub fn is_configured() -> bool {
    let s = settings();
    !s.child("aux").string("hw-name").is_empty() && !s.child("main").string("hw-name").is_empty()
}

pub fn load() -> SinkConfig {
    let s = settings();
    SinkConfig {
        aux:  load_sink(&s.child("aux")),
        main: load_sink(&s.child("main")),
    }
}

fn load_sink(s: &gio::Settings) -> SinkDef {
    SinkDef {
        channels:     s.int("channels") as u32,
        position:     s.string("position").into(),
        hw_name:      s.string("hw-name").into(),
        display_name: s.string("display-name").into(),
    }
}

pub fn store(cfg: &SinkConfig) {
    let s = settings();
    store_sink(&s.child("aux"), &cfg.aux);
    store_sink(&s.child("main"), &cfg.main);
}

fn store_sink(s: &gio::Settings, def: &SinkDef) {
    let _ = s.set_int("channels", def.channels as i32);
    let _ = s.set_string("position", &def.position);
    let _ = s.set_string("hw-name", &def.hw_name);
    let _ = s.set_string("display-name", &def.display_name);
}
