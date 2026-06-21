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

use std::path::PathBuf;

use crate::config::{Side, SinkConfig, SinkDef};

pub const AUX_SINK: &str = "dashboard_aux";
pub const MAIN_SINK: &str = "dashboard_main";
pub const AUX_PB: &str = "dashboard_aux_pb";
pub const MAIN_PB: &str = "dashboard_main_pb";

// Flatpak will need --filesystem=xdg-config/pipewire:create
pub fn config_dir() -> PathBuf {
    glib::home_dir().join(".config/pipewire/pipewire.conf.d")
}

pub fn config_file() -> PathBuf {
    config_dir().join("10-dashboard.conf")
}

/// Our main & aux loopback sinks; Routes to target hardware output or PW defeault
pub fn build_pw_config(cfg: &SinkConfig) -> String {
    let aux_body = loopback_body(Side::Aux, &cfg.aux, false);
    let main_body = loopback_body(Side::Main, &cfg.main, false);
    format!(
        r#"context.modules = [
  {{
    name = libpipewire-module-loopback
    args = {{
{aux_body}
    }}
  }}
  {{
    name = libpipewire-module-loopback
    args = {{
{main_body}
    }}
  }}
]
"#
    )
}

fn side_spec(side: Side) -> (&'static str, &'static str, &'static str) {
    match side {
        Side::Aux => (AUX_SINK, AUX_PB, "Dashboard - Aux"),
        Side::Main => (MAIN_SINK, MAIN_PB, "Dashboard - Main"),
    }
}

// The capture + playback props for one side. Shared by the persistent conf and
// the in-process module args so the two can't drift.
//
// Important: node.dont-fallback + node.linger are necessary to change WP's
// policy on fallback routing when the target.object disappears and preventing
// the node from being destroyed (e.g. HW output is unplugged)
//
// temp sinks add state.restore-props = false
// Avoids having recreated temp sinks desync volume/mute state with UI controls
fn loopback_body(side: Side, def: &SinkDef, temp: bool) -> String {
    let (name, pb_name, desc) = side_spec(side);
    let role = side.as_wire();
    let channels = def.channels;
    let position = def.position.replace(',', " ");
    let target = target_fragment(&def.hw_name);
    let restore = if temp {
        "\n        state.restore-props = false"
    } else {
        ""
    };
    format!(
        r#"      capture.props = {{
        node.name        = {name}
        node.description = "{desc}"
        media.class      = Audio/Sink
        audio.channels   = {channels}
        audio.position   = "[ {position} ]"
        node.virtual     = true{restore}
        dashboard.role  = {role}
      }}
      playback.props = {{
        node.name           = {pb_name}
        audio.channels      = {channels}
        audio.position      = "[ {position} ]"
        node.dont-fallback  = true
        node.linger         = true
        dashboard.pb-role  = {role}{target}
      }}"#
    )
}

// loopback args for pw_context_load_module; the same body the persistent conf
// uses plus the temp-sink state.restore-props = false
pub fn loopback_module_args(side: Side, def: &SinkDef) -> String {
    format!("{{\n{}\n}}", loopback_body(side, def, true))
}

/// Writes the pipewire.conf.d that persists the virtual sinks
pub fn write_config(cfg: &SinkConfig) {
    let file = config_file();
    if let Some(dir) = file.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("pw_config: failed to create {}: {e}", dir.display());
        return;
    }
    if let Err(e) = std::fs::write(&file, build_pw_config(cfg)) {
        eprintln!("pw_config: failed to write {}: {e}", file.display());
    }
}

/// Previews the conf files created in setup
pub fn preview_files(cfg: &SinkConfig) -> Vec<(String, String)> {
    vec![(
        config_file().to_string_lossy().into_owned(),
        build_pw_config(cfg),
    )]
}

// TODO: Revisit this when we get to runtime linking
fn target_fragment(hw_name: &str) -> String {
    if hw_name.is_empty() {
        String::new()
    } else {
        format!("\n        target.object  = \"{}\"", hw_name)
    }
}
