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

// The Routing tile

use std::collections::{BTreeMap, HashMap};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk4::{self as gtk};

use super::BridgeWindow;
use super::stream_list::{fill_streams, streams_popover};
use crate::audio::hw_sink::HwSink;
use crate::audio::routing::{
    RoutingRule, RuleTarget, StreamInfo, winning_rule_index, would_match_disabled_index,
};
use crate::config;
use crate::dialogs::add_rule;
use crate::util::{RouteTarget, route_targets, row_level_meter, stream_count};

struct RowBuild<'a> {
    text_group: &'a gtk::SizeGroup,
    action_group: &'a gtk::SizeGroup,
    meters: &'a mut Vec<(gtk::LevelBar, Vec<u32>)>,
    by_id: &'a HashMap<u32, StreamInfo>,
}

impl BridgeWindow {
    pub(super) fn toggle_routing_expanded(&self) {
        let expanded = !self.imp().routing_revealer.reveals_child();
        self.set_routing_expanded(expanded);
    }

    pub(super) fn set_routing_expanded(&self, expanded: bool) {
        let imp = self.imp();
        imp.routing_revealer.set_reveal_child(expanded);
        let icon = if expanded {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        };
        imp.routing_chevron.set_icon_name(Some(icon));
        imp.routing_toggle
            .update_state(&[gtk::accessible::State::Expanded(Some(expanded))]);
    }

    pub(super) fn refresh_routing_tile(&self) {
        let imp = self.imp();
        let Some(backend) = imp.backend.borrow().clone() else {
            return;
        };

        let rules = config::load_rules();
        let streams = backend.output_streams();
        let hw_sinks = backend.hw_sinks();

        imp.routing_add_button.set_sensitive(!streams.is_empty());

        // which live streams each rule governs
        // also the stremas without a governing rule
        // disabled rules still get matched to would-be streams
        let mut matched: Vec<Vec<u32>> = vec![Vec::new(); rules.len()];
        let mut unruled: BTreeMap<(Option<String>, Option<String>), Vec<u32>> = BTreeMap::new();
        for info in &streams {
            if let Some(idx) = winning_rule_index(&rules, info) {
                matched[idx].push(info.node_id);
            } else if let Some(idx) = would_match_disabled_index(&rules, info) {
                matched[idx].push(info.node_id);
            } else {
                let key = (info.app_name.clone(), info.binary.clone());
                unruled.entry(key).or_default().push(info.node_id);
            }
        }

        // the badge counts routing rules only
        let active = rules
            .iter()
            .zip(&matched)
            .filter(|(rule, ids)| rule.enabled && !ids.is_empty())
            .count();
        if active == 0 {
            imp.routing_badge.set_visible(false);
        } else {
            imp.routing_badge.set_label(&active.to_string());
            imp.routing_badge.set_visible(true);
        }

        while let Some(child) = imp.routing_body.first_child() {
            imp.routing_body.remove(&child);
        }
        let mut row_meters: Vec<(gtk::LevelBar, Vec<u32>)> = Vec::new();

        if rules.is_empty() && unruled.is_empty() {
            let empty = adw::ActionRow::new();
            empty.set_title("No streams playing right now");
            empty.set_subtitle("Start audio in an app to route it");
            empty.set_activatable(false);

            let card = gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .build();
            card.add_css_class("boxed-list");
            card.append(&empty);

            imp.routing_body.append(&card);
            imp.routing_row_meters.replace(row_meters);
            return;
        }

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .build();
        list.add_css_class("boxed-list");

        // horizontal size groups to keep alignment between entries stable
        let text_group = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        let action_group = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        let by_id: HashMap<u32, StreamInfo> =
            streams.iter().map(|s| (s.node_id, s.clone())).collect();
        let mut build = RowBuild {
            text_group: &text_group,
            action_group: &action_group,
            meters: &mut row_meters,
            by_id: &by_id,
        };

        // rules and streams share one list ordered by live activity
        enum Item {
            Rule(usize),
            Stream((Option<String>, Option<String>), Vec<u32>),
        }
        let mut items: Vec<Item> = (0..rules.len()).map(Item::Rule).collect();
        for (key, ids) in unruled {
            items.push(Item::Stream(key, ids));
        }
        // Group by live activity, sorted alphabetically
        items.sort_by_key(|item| match item {
            Item::Rule(idx) => {
                let group = if matched[*idx].is_empty() { 1 } else { 0 };
                (group, rules[*idx].display_name.to_lowercase())
            }
            Item::Stream((app, binary), _) => {
                let title = app.clone().or_else(|| binary.clone()).unwrap_or_default();
                (0, title.to_lowercase())
            }
        });
        for item in items {
            let row = match item {
                Item::Rule(idx) => {
                    self.build_rule_row(idx, &rules[idx], &hw_sinks, &matched[idx], &mut build)
                }
                Item::Stream((app, binary), ids) => {
                    self.build_stream_row(app, binary, ids, &mut build)
                }
            };
            list.append(&row);
        }

        imp.routing_body.append(&list);
        imp.routing_row_meters.replace(row_meters);
        imp.routing_size_groups
            .replace(vec![text_group, action_group]);
    }

    fn build_rule_row(
        &self,
        idx: usize,
        rule: &RoutingRule,
        hw_sinks: &[HwSink],
        ids: &[u32],
        build: &mut RowBuild,
    ) -> gtk::ListBoxRow {
        let count = ids.len();

        let (glyph, dot_class) = match (rule.enabled, count > 0) {
            (true, true) => ("●", "rule-dot-active"),
            (true, false) => ("●", "rule-dot-idle"),
            (false, _) => ("○", "rule-dot-disabled"),
        };
        let row = row_shell(glyph, dot_class);

        let subtitle = count_subtitle(&rule_subtitle(rule), group_streams(ids, build.by_id));
        let text = row_text_block(&rule.display_name, &subtitle);
        build.text_group.add_widget(&text);
        if !rule.enabled {
            text.set_opacity(0.55);
        }
        row.append(&text);

        let arrow = route_arrow();
        let (model, selected) = target_choices(hw_sinks, &rule.target);
        let chip = gtk::DropDown::builder()
            .model(&model)
            .factory(&target_chip_factory())
            .valign(gtk::Align::Center)
            .build();
        if !rule.enabled {
            arrow.set_opacity(0.55);
            chip.set_opacity(0.55);
        }
        row.append(&arrow);
        chip.set_selected(selected);
        chip.connect_selected_notify(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |d| {
                if let Some(target) = selected_target(d) {
                    w.on_rule_retargeted(idx, target);
                }
            }
        ));
        row.append(&chip);

        row.append(&row_spacer());

        // only show level meter when there is activity
        if !ids.is_empty() {
            let meter = row_level_meter();
            row.append(&meter);
            build.meters.push((meter, ids.to_vec()));
        }

        let switch = gtk::Switch::builder()
            .active(rule.enabled)
            .valign(gtk::Align::Center)
            .build();
        switch.connect_active_notify(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |s| w.on_rule_toggled(idx, s.is_active())
        ));

        let popover = gtk::Popover::new();
        let delete_btn = gtk::Button::with_label("Delete");
        delete_btn.add_css_class("flat");
        delete_btn.add_css_class("destructive-action");
        delete_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                w.on_rule_deleted(idx);
            }
        ));
        popover.set_child(Some(&delete_btn));

        let menu_btn = gtk::MenuButton::new();
        menu_btn.set_icon_name("view-more-symbolic");
        menu_btn.add_css_class("flat");
        menu_btn.set_valign(gtk::Align::Center);
        menu_btn.set_popover(Some(&popover));

        row.append(&action_column(
            &[switch.upcast_ref(), menu_btn.upcast_ref()],
            build.action_group,
        ));

        wrap_row(&row)
    }

    fn on_rule_retargeted(&self, idx: usize, target: RuleTarget) {
        let mut rules = config::load_rules();
        let Some(r) = rules.get_mut(idx) else { return };
        if r.target == target {
            return;
        }
        r.target = target;
        self.store_and_apply(rules);
    }

    fn on_rule_toggled(&self, idx: usize, enabled: bool) {
        let mut rules = config::load_rules();
        let Some(r) = rules.get_mut(idx) else { return };
        if r.enabled == enabled {
            return;
        }
        r.enabled = enabled;
        self.store_and_apply(rules);
    }

    fn on_rule_deleted(&self, idx: usize) {
        let mut rules = config::load_rules();
        if idx >= rules.len() {
            return;
        }
        rules.remove(idx);
        self.store_and_apply(rules);
    }

    fn store_and_apply(&self, rules: Vec<RoutingRule>) {
        config::store_rules(&rules);
        if let Some(backend) = self.imp().backend.borrow().clone() {
            backend.apply_rules_all();
        }
        self.refresh_routing_tile();
    }

    fn build_stream_row(
        &self,
        app_name: Option<String>,
        binary: Option<String>,
        ids: Vec<u32>,
        build: &mut RowBuild,
    ) -> gtk::ListBoxRow {
        let had_app_name = app_name.is_some();
        let title = app_name.or_else(|| binary.clone()).unwrap_or_default();

        let prefix = if had_app_name {
            binary.unwrap_or_default()
        } else {
            String::new()
        };

        let row = row_shell("○", "rule-dot-live");
        let subtitle = count_subtitle(&prefix, group_streams(&ids, build.by_id));
        let text = row_text_block(&title, &subtitle);
        build.text_group.add_widget(&text);
        row.append(&text);
        row.append(&row_spacer());

        let meter = row_level_meter();
        row.append(&meter);
        build.meters.push((meter, ids.clone()));

        let add_btn = gtk::Button::with_label("Add Rule");
        add_btn.add_css_class("flat");
        add_btn.set_valign(gtk::Align::Center);
        add_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = w)]
            self,
            move |_| w.show_add_rule_dialog(&ids)
        ));
        row.append(&action_column(&[add_btn.upcast_ref()], build.action_group));

        wrap_row(&row)
    }

    pub(super) fn show_add_rule_dialog(&self, preselect: &[u32]) {
        let Some(backend) = self.imp().backend.borrow().clone() else {
            return;
        };
        let streams = backend.output_streams();
        let hw_sinks = backend.hw_sinks();

        self.imp().stream_meters_paused.set(true);
        let dialog = add_rule::show(
            Some(self),
            streams,
            hw_sinks,
            preselect,
            backend,
            glib::clone!(
                #[weak(rename_to = w)]
                self,
                move |rule| {
                    let mut rules = config::load_rules();
                    rules.push(rule);
                    w.store_and_apply(rules);
                }
            ),
        );
        dialog.connect_close_request(glib::clone!(
            #[weak(rename_to = w)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_| {
                w.imp().stream_meters_paused.set(false);
                glib::Propagation::Proceed
            }
        ));
    }
}

fn rule_subtitle(rule: &RoutingRule) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(p) = match_summary(&rule.match_app_names) {
        parts.push(p);
    }
    if let Some(p) = match_summary(&rule.match_binaries) {
        parts.push(p);
    }
    if parts.is_empty() {
        "(no constraints)".to_owned()
    } else {
        parts.join(" · ")
    }
}

fn group_streams(ids: &[u32], by_id: &HashMap<u32, StreamInfo>) -> Vec<StreamInfo> {
    ids.iter().filter_map(|id| by_id.get(id).cloned()).collect()
}

fn match_summary(values: &[String]) -> Option<String> {
    match values {
        [] => None,
        [one] => Some(one.clone()),
        many => Some(format!("one of {}", many.len())),
    }
}

fn target_choices(hw_sinks: &[HwSink], current: &RuleTarget) -> (gio::ListStore, u32) {
    let mut choices = route_targets(hw_sinks);

    if let RuleTarget::DirectHw(name) = current
        && !choices.iter().any(|c| &c.target == current)
    {
        choices.push(RouteTarget {
            label: name.clone(),
            target: current.clone(),
        });
    }
    let selected = choices
        .iter()
        .position(|c| &c.target == current)
        .unwrap_or(0) as u32;

    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    for choice in choices {
        store.append(&glib::BoxedAnyObject::new(choice));
    }
    (store, selected)
}

fn selected_target(dropdown: &gtk::DropDown) -> Option<RuleTarget> {
    dropdown
        .selected_item()
        .and_downcast::<glib::BoxedAnyObject>()
        .map(|boxed| boxed.borrow::<RouteTarget>().target.clone())
}

fn target_chip_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(|_, obj| {
        let item = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(12)
            .xalign(0.0)
            .build();
        item.set_child(Some(&label));
    });

    factory.connect_bind(|_, obj| {
        let item = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let label = item.child().unwrap().downcast::<gtk::Label>().unwrap();
        if let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() {
            label.set_label(&boxed.borrow::<RouteTarget>().label);
        }
    });
    factory
}

fn row_shell(dot_glyph: &str, dot_class: &str) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    let dot = gtk::Label::new(Some(dot_glyph));
    dot.add_css_class(dot_class);
    dot.set_valign(gtk::Align::Center);
    row.append(&dot);
    row
}

fn row_text_block(title: &str, subtitle: &impl IsA<gtk::Widget>) -> gtk::Box {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .build();
    let name = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(20)
        .build();
    text.append(&name);
    text.append(subtitle);
    text
}

// The subtitle line constructor
fn count_subtitle(prefix: &str, streams: Vec<StreamInfo>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    if !prefix.is_empty() {
        let p = gtk::Label::builder()
            .label(prefix)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(24)
            .build();
        p.add_css_class("dim-label");
        p.add_css_class("caption");
        row.append(&p);
    }

    if streams.is_empty() {
        return row;
    }

    if !prefix.is_empty() {
        let sep = gtk::Label::new(Some(" · "));
        sep.add_css_class("dim-label");
        sep.add_css_class("caption");
        row.append(&sep);
    }

    let (popover, list) = streams_popover("Streams Playing");
    let label = gtk::Label::new(Some(&stream_count(streams.len())));
    label.add_css_class("caption");

    popover.connect_map(move |_| fill_streams(&list, &streams));

    let button = gtk::MenuButton::builder()
        .child(&label)
        .valign(gtk::Align::Center)
        .always_show_arrow(false)
        .popover(&popover)
        .build();
    button.add_css_class("flat");
    button.add_css_class("count-button");
    row.append(&button);

    row
}

fn route_arrow() -> gtk::Label {
    let arrow = gtk::Label::new(Some("→"));
    arrow.add_css_class("dim-label");
    arrow.set_valign(gtk::Align::Center);
    arrow
}

fn row_spacer() -> gtk::Box {
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    spacer
}

fn action_column(children: &[&gtk::Widget], group: &gtk::SizeGroup) -> gtk::Box {
    let col = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .hexpand(false)
        .build();
    col.append(&row_spacer());
    for child in children {
        col.append(*child);
    }
    group.add_widget(&col);
    col
}

fn wrap_row(content: &gtk::Box) -> gtk::ListBoxRow {
    gtk::ListBoxRow::builder()
        .activatable(false)
        .selectable(false)
        .child(content)
        .build()
}
