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

use crate::config::SinkConfig;

pub const AUX_SINK:  &str = "dashboard_aux";
pub const MAIN_SINK: &str = "dashboard_main";

// Flatpak will need --firesystem=xdg-config/pipewire:create
pub fn config_dir() -> PathBuf {
    glib::home_dir().join(".config/pipewire/pipewire.conf.d")
}

pub fn config_file() -> PathBuf {
    config_dir().join("10-dashboard.conf")
}

/// Our main & aux loopback sinks; Routes to target hardware output or PW defeault
pub fn build_pw_config(cfg: &SinkConfig) -> String {
    let aux_channels  = cfg.aux.channels;
    let main_channels = cfg.main.channels;
    let aux_position  = cfg.aux.position.replace(',', " ");
    let main_position = cfg.main.position.replace(',', " ");
    let aux_target  = target_fragment(&cfg.aux.hw_name);
    let main_target = target_fragment(&cfg.main.hw_name);
    let aux_name  = AUX_SINK;
    let main_name = MAIN_SINK;

    // Important: node.dont-fallback + node.linger are necessary to change WP's
    // policy on fallback routing when the target.object disappears and preventing
    // the node from being destroyed (e.g. HW output is unplugged)
    format!(
        r#"context.modules = [
  {{
    name = libpipewire-module-loopback
    args = {{
      capture.props = {{
        node.name        = {aux_name}
        node.description = "Dashboard - Aux"
        media.class      = Audio/Sink
        audio.channels   = {aux_channels}
        audio.position   = "[ {aux_position} ]"
        node.virtual     = true
        dashboard.role  = aux
      }}
      playback.props = {{
        node.name           = dashboard_aux_pb
        audio.channels      = {aux_channels}
        audio.position      = "[ {aux_position} ]"
        node.dont-fallback  = true
        node.linger         = true
        dashboard.pb-role  = aux{aux_target}
      }}
    }}
  }}
  {{
    name = libpipewire-module-loopback
    args = {{
      capture.props = {{
        node.name        = {main_name}
        node.description = "Dashboard - Main"
        media.class      = Audio/Sink
        audio.channels   = {main_channels}
        audio.position   = "[ {main_position} ]"
        node.virtual     = true
        dashboard.role  = main
      }}
      playback.props = {{
        node.name           = dashboard_main_pb
        audio.channels      = {main_channels}
        audio.position      = "[ {main_position} ]"
        node.dont-fallback  = true
        node.linger         = true
        dashboard.pb-role  = main{main_target}
      }}
    }}
  }}
]
"#
    )
}

/// Writes the pipewire.conf.d that persists the virtual sinks
pub fn write_config(cfg: &SinkConfig) {
    let file = config_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&file, build_pw_config(cfg));
}

/// Previews the conf files created in setup
pub fn preview_files(cfg: &SinkConfig) -> Vec<(String, String)> {
    vec![(config_file().to_string_lossy().into_owned(), build_pw_config(cfg))]
}

// TODO: Revisit this when we get to runtime linking
fn target_fragment(hw_name: &str) -> String {
    if hw_name.is_empty() {
        String::new()
    } else {
        format!("\n        target.object  = \"{}\"", hw_name)
    }
}
