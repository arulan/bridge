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

use crate::config::Side;

// How we address the nodes we create; Side was too limiting
// Role can address all of the virtual devices
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Aux,
    Main,
    Surround,
}

impl Role {
    /// Every virtual device
    pub const ALL: [Role; 3] = [Role::Aux, Role::Main, Role::Surround];

    /// Parse bridge.role from the loopback conf
    pub fn from_wire(s: &str) -> Option<Role> {
        match s {
            "aux" => Some(Role::Aux),
            "main" => Some(Role::Main),
            "surround" => Some(Role::Surround),
            _ => None,
        }
    }

    /// bridge.role string for the loopback conf
    pub fn as_wire(&self) -> &'static str {
        match self {
            Role::Aux => "aux",
            Role::Main => "main",
            Role::Surround => "surround",
        }
    }
}

impl From<Side> for Role {
    fn from(side: Side) -> Self {
        match side {
            Side::Aux => Role::Aux,
            Side::Main => Role::Main,
        }
    }
}
