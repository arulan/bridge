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

use crate::application::settings;
use crate::audio::hw_sink::HwSink;
use crate::audio::routing::RoutingRule;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Aux,
    Main,
}

impl Side {
    /// Parse dashboard.role from the loopback conf
    pub fn from_wire(s: &str) -> Option<Side> {
        match s {
            "aux" => Some(Side::Aux),
            "main" => Some(Side::Main),
            _ => None,
        }
    }

    /// dashboard.role string for the loopback conf
    pub fn as_wire(&self) -> &'static str {
        match self {
            Side::Aux => "aux",
            Side::Main => "main",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SinkDef {
    pub channels: u32,
    pub position: String,
    pub hw_name: String,
    pub display_name: String,
}

#[derive(Clone, Debug)]
pub struct SinkConfig {
    pub aux: SinkDef,
    pub main: SinkDef,
}

#[derive(Clone, Debug, Default)]
pub struct SurroundConfig {
    pub hrir_path: String,
    pub hw_name: String,
    pub display_name: String,
}

impl SinkConfig {
    pub fn side(&self, side: Side) -> &SinkDef {
        match side {
            Side::Aux => &self.aux,
            Side::Main => &self.main,
        }
    }

    pub fn side_mut(&mut self, side: Side) -> &mut SinkDef {
        match side {
            Side::Aux => &mut self.aux,
            Side::Main => &mut self.main,
        }
    }
}

impl From<HwSink> for SinkDef {
    fn from(sink: HwSink) -> Self {
        SinkDef {
            channels: sink.channels,
            position: sink.position,
            hw_name: sink.name,
            display_name: sink.display_name,
        }
    }
}

// true after first-run setup
pub fn is_configured() -> bool {
    let s = settings();
    !s.child("aux").string("hw-name").is_empty() && !s.child("main").string("hw-name").is_empty()
}

pub fn load() -> SinkConfig {
    let s = settings();
    SinkConfig {
        aux: load_sink(&s.child("aux")),
        main: load_sink(&s.child("main")),
    }
}

fn load_sink(s: &gio::Settings) -> SinkDef {
    SinkDef {
        channels: s.int("channels") as u32,
        position: s.string("position").into(),
        hw_name: s.string("hw-name").into(),
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

// Clears the Aux/Main output settings; next launch falls back to first-run setup
pub fn clear_sinks() {
    let s = settings();
    for side in ["aux", "main"] {
        let c = s.child(side);
        c.reset("channels");
        c.reset("position");
        c.reset("hw-name");
        c.reset("display-name");
    }
}

pub fn surround_enabled() -> bool {
    !settings().child("surround").string("hrir-path").is_empty()
}

pub fn load_surround() -> SurroundConfig {
    let s = settings().child("surround");
    SurroundConfig {
        hrir_path: s.string("hrir-path").into(),
        hw_name: s.string("hw-name").into(),
        display_name: s.string("display-name").into(),
    }
}

pub fn store_surround(cfg: &SurroundConfig) {
    let s = settings().child("surround");
    let _ = s.set_string("hrir-path", &cfg.hrir_path);
    let _ = s.set_string("hw-name", &cfg.hw_name);
    let _ = s.set_string("display-name", &cfg.display_name);
}

// Resets the virtual surround configuration
pub fn clear_surround() {
    let s = settings().child("surround");
    s.reset("hrir-path");
    s.reset("hw-name");
    s.reset("display-name");
    s.reset("active");
}

pub fn surround_active() -> bool {
    settings().child("surround").boolean("active")
}

pub fn set_surround_active(active: bool) {
    let _ = settings().child("surround").set_boolean("active", active);
}

pub fn default_follows_main() -> bool {
    settings().boolean("default-follows-main")
}

pub fn set_default_follows_main(follows: bool) {
    let _ = settings().set_boolean("default-follows-main", follows);
}

pub fn keep_routing_open() -> bool {
    settings().boolean("keep-routing-open")
}

pub fn set_keep_routing_open(open: bool) {
    let _ = settings().set_boolean("keep-routing-open", open);
}

pub const CROSSFADE_STEP_MIN: i32 = 2;
pub const CROSSFADE_STEP_MAX: i32 = 25;

pub fn crossfade_step() -> i32 {
    settings().int("crossfade-step")
}

pub fn set_crossfade_step(percent: i32) {
    let _ = settings().set_int("crossfade-step", percent);
}

// Routing rules live in a GVariant array key
pub fn load_rules() -> Vec<RoutingRule> {
    settings()
        .value("rules")
        .iter()
        .filter_map(|v| RoutingRule::from_variant(&v))
        .collect()
}

pub fn store_rules(rules: &[RoutingRule]) {
    let _ = settings().set_value("rules", &rules.to_variant());
}

// Quick Switch presets
// A preset saves the hardware target of both Aux and Main
// and is given a name
// "" for aux_hw/main_hw means leave side unchanged on switch
// id for future reference if this expands beyond Quick Switch
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub aux_hw: String,
    pub main_hw: String,
}

impl Preset {
    pub fn new() -> Self {
        Preset {
            id: glib::uuid_string_random().into(),
            ..Default::default()
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.aux_hw.is_empty() || !self.main_hw.is_empty()
    }

    pub fn matches(&self, aux_hw: &str, main_hw: &str) -> bool {
        (self.aux_hw.is_empty() || self.aux_hw == aux_hw)
            && (self.main_hw.is_empty() || self.main_hw == main_hw)
    }

    fn to_dict(&self) -> glib::VariantDict {
        let dict = glib::VariantDict::new(None);
        dict.insert_value("id", &self.id.to_variant());
        dict.insert_value("name", &self.name.to_variant());
        dict.insert_value("aux-hw", &self.aux_hw.to_variant());
        dict.insert_value("main-hw", &self.main_hw.to_variant());
        dict
    }

    fn from_variant(v: &glib::Variant) -> Option<Self> {
        let dict = glib::VariantDict::new(Some(v));
        let get = |key| dict.lookup::<String>(key).ok().flatten().unwrap_or_default();
        Some(Preset {
            id: get("id"),
            name: get("name"),
            aux_hw: get("aux-hw"),
            main_hw: get("main-hw"),
        })
    }
}

pub fn load_presets() -> Vec<Preset> {
    settings()
        .value("presets")
        .iter()
        .filter_map(|v| Preset::from_variant(&v))
        .collect()
}

pub fn store_presets(presets: &[Preset]) {
    let dicts: Vec<glib::VariantDict> = presets.iter().map(Preset::to_dict).collect();
    let _ = settings().set_value("presets", &dicts.to_variant());
}

// True if at two presets exist, meaning Quick Switch is configured
// TODO: If we add other writers of presets later, this will have to change
pub fn presets_configured() -> bool {
    load_presets().iter().filter(|p| p.is_valid()).count() >= 2
}
