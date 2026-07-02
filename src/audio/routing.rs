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
    pub match_app_name: Option<String>,
    pub match_binary: Option<String>,
    pub target: RuleTarget,
    pub enabled: bool,
}

impl RoutingRule {
    /// Higher specificity (more properties) wins ties
    pub fn specificity(&self) -> usize {
        self.match_app_name.is_some() as usize + self.match_binary.is_some() as usize
    }

    pub fn matches(&self, info: &StreamInfo) -> bool {
        if let Some(n) = &self.match_app_name
            && info.app_name.as_deref() != Some(n.as_str())
        {
            return false;
        }
        if let Some(b) = &self.match_binary
            && info.binary.as_deref() != Some(b.as_str())
        {
            return false;
        }
        self.specificity() > 0
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
    rules
        .iter()
        .enumerate()
        .filter(|(_, r)| r.enabled)
        .filter(|(_, r)| r.matches(info))
        .max_by_key(|(_, r)| r.specificity())
        .map(|(idx, _)| idx)
}
