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

// Routing rule types and matcher

use std::borrow::Cow;

use glib::VariantTy;
use glib::variant::{FromVariant, StaticVariantType, ToVariant};

use super::pw_config::{AUX_SINK, MAIN_SINK};

#[derive(Clone, Debug, PartialEq)]
pub enum RuleTarget {
    Aux,
    Main,
    DirectHw(String),
}

impl RuleTarget {
    pub(crate) fn serialize(&self) -> String {
        match self {
            RuleTarget::Aux => "aux".to_owned(),
            RuleTarget::Main => "main".to_owned(),
            RuleTarget::DirectHw(n) => format!("hw:{}", n),
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "aux" => Some(RuleTarget::Aux),
            "main" => Some(RuleTarget::Main),
            other => other
                .strip_prefix("hw:")
                .filter(|n| !n.is_empty())
                .map(|n| RuleTarget::DirectHw(n.to_owned())),
        }
    }

    /// The node a matching stream should target to.
    pub(crate) fn node_name(&self) -> String {
        match self {
            RuleTarget::Aux => AUX_SINK.to_owned(),
            RuleTarget::Main => MAIN_SINK.to_owned(),
            RuleTarget::DirectHw(n) => n.clone(),
        }
    }
}

impl StaticVariantType for RuleTarget {
    fn static_variant_type() -> Cow<'static, VariantTy> {
        Cow::Borrowed(VariantTy::STRING)
    }
}

impl ToVariant for RuleTarget {
    fn to_variant(&self) -> glib::Variant {
        self.serialize().to_variant()
    }
}

impl FromVariant for RuleTarget {
    fn from_variant(variant: &glib::Variant) -> Option<Self> {
        RuleTarget::parse(variant.str()?)
    }
}

impl From<RuleTarget> for glib::Variant {
    fn from(target: RuleTarget) -> Self {
        target.to_variant()
    }
}

#[derive(Clone, Debug, glib::Variant)]
pub struct RoutingRule {
    pub display_name: String,
    pub match_app_names: Vec<String>,
    pub match_binaries: Vec<String>,
    pub target: RuleTarget,
    pub enabled: bool,
}

impl RoutingRule {
    /// Higher specificity (more properties) wins ties
    pub fn specificity(&self) -> usize {
        (!self.match_app_names.is_empty()) as usize + (!self.match_binaries.is_empty()) as usize
    }

    pub fn matches(&self, info: &StreamInfo) -> bool {
        if !self.match_app_names.is_empty() && !contains(&self.match_app_names, &info.app_name) {
            return false;
        }
        if !self.match_binaries.is_empty() && !contains(&self.match_binaries, &info.binary) {
            return false;
        }
        self.specificity() > 0
    }
}

fn contains(set: &[String], value: &Option<String>) -> bool {
    match value {
        Some(v) => set.iter().any(|s| s == v),
        None => false,
    }
}

#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub node_id: u32,
    pub app_name: Option<String>,
    pub app_icon: Option<String>,
    pub binary: Option<String>,
    pub media_name: Option<String>,
}

pub fn winning_rule_index(rules: &[RoutingRule], info: &StreamInfo) -> Option<usize> {
    best_match_index(rules, info, true)
}

// keeps streams attached to the disabled rule that would match it
pub fn would_match_disabled_index(rules: &[RoutingRule], info: &StreamInfo) -> Option<usize> {
    best_match_index(rules, info, false)
}

fn best_match_index(rules: &[RoutingRule], info: &StreamInfo, enabled: bool) -> Option<usize> {
    rules
        .iter()
        .enumerate()
        .filter(|(_, r)| r.enabled == enabled)
        .filter(|(_, r)| r.matches(info))
        .max_by_key(|(_, r)| r.specificity())
        .map(|(idx, _)| idx)
}
