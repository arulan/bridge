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

use std::path::{Path, PathBuf};

use crate::config::{Side, SinkConfig, SinkDef};

pub const AUX_SINK: &str = "dashboard_aux";
pub const MAIN_SINK: &str = "dashboard_main";
pub const AUX_PB: &str = "dashboard_aux_pb";
pub const MAIN_PB: &str = "dashboard_main_pb";

pub const SURROUND_SINK: &str = "dashboard_surround";

// Flatpak will need --filesystem=xdg-config/pipewire:create
pub fn config_dir() -> PathBuf {
    glib::home_dir().join(".config/pipewire/pipewire.conf.d")
}

pub fn config_file() -> PathBuf {
    config_dir().join("10-dashboard.conf")
}

// Separate conf from the Main/Aux virtual sinks
pub fn surround_config_file() -> PathBuf {
    config_dir().join("10-dashboard-surround.conf")
}

pub fn hrir_dir() -> PathBuf {
    glib::home_dir().join(".config/pipewire/hrir")
}

// Keeps a copy of the user-selected HRIR to the standard PW dir
pub fn import_hrir(src: &Path) -> std::io::Result<PathBuf> {
    let dir = hrir_dir();
    std::fs::create_dir_all(&dir)?;
    let name = src.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "HRIR has no filename")
    })?;
    let dest = dir.join(name);
    if src != dest {
        std::fs::copy(src, &dest)?;
    }
    Ok(dest)
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
pub fn write_config(cfg: &SinkConfig) -> std::io::Result<()> {
    let file = config_file();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, build_pw_config(cfg))
}

// Drops the Aux/Main conf; virtual sinks are removed after next login
pub fn remove_config() {
    let file = config_file();
    if let Err(e) = std::fs::remove_file(&file)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("pw_config: failed to remove {}: {e}", file.display());
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

// HeSuVi 14-channel WAV to stereo HRTF convolver graph
const SURROUND_TEMPLATE: &str = r#"context.modules = [
  {
    name = libpipewire-module-filter-chain
    flags = [ nofail ]
    args = {
      node.description = "Dashboard - Virtual Surround"
      media.name       = "Dashboard - Virtual Surround"
      filter.graph = {
        nodes = [
          { type = builtin label = copy name = copyFL  }
          { type = builtin label = copy name = copyFR  }
          { type = builtin label = copy name = copyFC  }
          { type = builtin label = copy name = copyRL  }
          { type = builtin label = copy name = copyRR  }
          { type = builtin label = copy name = copySL  }
          { type = builtin label = copy name = copySR  }
          { type = builtin label = copy name = copyLFE }

          { type = builtin label = convolver name = convFL_L  config = { filename = "{hrir}" channel =  0 } }
          { type = builtin label = convolver name = convFL_R  config = { filename = "{hrir}" channel =  1 } }
          { type = builtin label = convolver name = convSL_L  config = { filename = "{hrir}" channel =  2 } }
          { type = builtin label = convolver name = convSL_R  config = { filename = "{hrir}" channel =  3 } }
          { type = builtin label = convolver name = convRL_L  config = { filename = "{hrir}" channel =  4 } }
          { type = builtin label = convolver name = convRL_R  config = { filename = "{hrir}" channel =  5 } }
          { type = builtin label = convolver name = convFC_L  config = { filename = "{hrir}" channel =  6 } }
          { type = builtin label = convolver name = convFR_R  config = { filename = "{hrir}" channel =  7 } }
          { type = builtin label = convolver name = convFR_L  config = { filename = "{hrir}" channel =  8 } }
          { type = builtin label = convolver name = convSR_R  config = { filename = "{hrir}" channel =  9 } }
          { type = builtin label = convolver name = convSR_L  config = { filename = "{hrir}" channel = 10 } }
          { type = builtin label = convolver name = convRR_R  config = { filename = "{hrir}" channel = 11 } }
          { type = builtin label = convolver name = convRR_L  config = { filename = "{hrir}" channel = 12 } }
          { type = builtin label = convolver name = convFC_R  config = { filename = "{hrir}" channel = 13 } }

          { type = builtin label = convolver name = convLFE_L config = { filename = "{hrir}" channel =  6 } }
          { type = builtin label = convolver name = convLFE_R config = { filename = "{hrir}" channel = 13 } }

          { type = builtin label = mixer name = mixL }
          { type = builtin label = mixer name = mixR }
        ]
        links = [
          { output = "copyFL:Out"  input="convFL_L:In"  }
          { output = "copyFL:Out"  input="convFL_R:In"  }
          { output = "copySL:Out"  input="convSL_L:In"  }
          { output = "copySL:Out"  input="convSL_R:In"  }
          { output = "copyRL:Out"  input="convRL_L:In"  }
          { output = "copyRL:Out"  input="convRL_R:In"  }
          { output = "copyFC:Out"  input="convFC_L:In"  }
          { output = "copyFR:Out"  input="convFR_R:In"  }
          { output = "copyFR:Out"  input="convFR_L:In"  }
          { output = "copySR:Out"  input="convSR_R:In"  }
          { output = "copySR:Out"  input="convSR_L:In"  }
          { output = "copyRR:Out"  input="convRR_R:In"  }
          { output = "copyRR:Out"  input="convRR_L:In"  }
          { output = "copyFC:Out"  input="convFC_R:In"  }
          { output = "copyLFE:Out" input="convLFE_L:In" }
          { output = "copyLFE:Out" input="convLFE_R:In" }

          { output = "convFL_L:Out"  input="mixL:In 1" }
          { output = "convFL_R:Out"  input="mixR:In 1" }
          { output = "convSL_L:Out"  input="mixL:In 2" }
          { output = "convSL_R:Out"  input="mixR:In 2" }
          { output = "convRL_L:Out"  input="mixL:In 3" }
          { output = "convRL_R:Out"  input="mixR:In 3" }
          { output = "convFC_L:Out"  input="mixL:In 4" }
          { output = "convFC_R:Out"  input="mixR:In 4" }
          { output = "convFR_R:Out"  input="mixR:In 5" }
          { output = "convFR_L:Out"  input="mixL:In 5" }
          { output = "convSR_R:Out"  input="mixR:In 6" }
          { output = "convSR_L:Out"  input="mixL:In 6" }
          { output = "convRR_R:Out"  input="mixR:In 7" }
          { output = "convRR_L:Out"  input="mixL:In 7" }
          { output = "convLFE_R:Out" input="mixR:In 8" }
          { output = "convLFE_L:Out" input="mixL:In 8" }
        ]
        inputs  = [ "copyFL:In" "copyFR:In" "copyFC:In" "copyLFE:In" "copyRL:In" "copyRR:In" "copySL:In" "copySR:In" ]
        outputs = [ "mixL:Out" "mixR:Out" ]
      }
      capture.props = {
        node.name        = dashboard_surround
        node.description = "Dashboard - Virtual Surround"
        media.class      = Audio/Sink
        audio.channels   = 8
        audio.position   = "[ FL FR FC LFE RL RR SL SR ]"
        node.virtual     = true
        dashboard.role  = surround
      }
      playback.props = {
        node.name           = dashboard_surround_pb
        audio.channels      = 2
        audio.position      = "[ FL FR ]"
        node.dont-fallback  = true
        node.linger         = true
        dashboard.pb-role  = surround{hw_target}
      }
    }
  }
]
"#;

pub fn build_surround_pw_config(hrir_path: &str, hw_name: &str) -> String {
    let hw_target = if hw_name.is_empty() {
        String::new()
    } else {
        format!("\n        target.object       = \"{}\"", hw_name)
    };
    SURROUND_TEMPLATE
        .replace("{hrir}", hrir_path)
        .replace("{hw_target}", &hw_target)
}

pub fn surround_preview_files(hrir_path: &str, hw_name: &str) -> Vec<(String, String)> {
    vec![(
        surround_config_file().to_string_lossy().into_owned(),
        build_surround_pw_config(hrir_path, hw_name),
    )]
}

pub fn write_surround_config(hrir_path: &str, hw_name: &str) -> std::io::Result<()> {
    let file = surround_config_file();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, build_surround_pw_config(hrir_path, hw_name))
}

pub fn remove_surround_config() {
    let file = surround_config_file();
    if let Err(e) = std::fs::remove_file(&file)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("pw_config: failed to remove {}: {e}", file.display());
    }
}
