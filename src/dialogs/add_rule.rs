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

// Add Routing Rule dialog
// Pick one or more live streams to build up a rule
// application.name and application.process.binary are our current rule properties to match on

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk4::{self as gtk};

use crate::audio::backend::PipeWireBackend;
use crate::audio::hw_sink::HwSink;
use crate::audio::routing::{RoutingRule, StreamInfo};
use crate::util::{RouteTarget, drive_stream_meters, route_targets, row_level_meter, stream_count};

// streams that share a common application.name & application.process.binary identity
struct Group {
    title: String,
    binary: Option<String>,
    icon: String,
    // indices into Ui::streams
    stream_idx: Vec<usize>,
    media_names: Vec<String>,
}

// per-group widgets we flip when the row's checked state changes
struct RowWidgets {
    row: gtk::ListBoxRow,
    check: gtk::Image,
    // when the row is selected, reveal individual streams of the group
    revealer: gtk::Revealer,
}

struct Ui {
    // every live output stream, sorted
    streams: Vec<StreamInfo>,
    groups: Vec<Group>,
    checked: Vec<Cell<bool>>,
    rows: Vec<RowWidgets>,

    name_switch: gtk::Switch,
    bin_switch: gtk::Switch,
    name_row: adw::ActionRow,
    bin_row: adw::ActionRow,
    match_list: gtk::ListBox,
    match_group: adw::PreferencesGroup,
    warning: gtk::Box,
    warning_caption: gtk::Label,

    route_options: Vec<RouteTarget>,
    route_combo: adw::ComboRow,

    name_entry: adw::EntryRow,
    user_edited: Cell<bool>,
    suggesting: Cell<bool>,

    preview_group: adw::PreferencesGroup,
    preview_list: gtk::ListBox,

    add_button: gtk::Button,
}

impl Ui {
    fn selected_streams(&self) -> Vec<&StreamInfo> {
        let mut out = Vec::new();
        for (g, checked) in self.groups.iter().zip(&self.checked) {
            if checked.get() {
                out.extend(g.stream_idx.iter().map(|&i| &self.streams[i]));
            }
        }
        out
    }

    // the rule the current select + toggle switches would produce, or None if not valid
    fn rule_from(&self, names: &[String], bins: &[String]) -> Option<RoutingRule> {
        if !shared_identity(names, bins) {
            return None;
        }

        let match_app_names = if !names.is_empty() && self.name_switch.is_active() {
            names.to_vec()
        } else {
            Vec::new()
        };
        let match_binaries = if !bins.is_empty() && self.bin_switch.is_active() {
            bins.to_vec()
        } else {
            Vec::new()
        };
        if match_app_names.is_empty() && match_binaries.is_empty() {
            return None;
        }

        let display_name = self.name_entry.text().trim().to_owned();
        if display_name.is_empty() {
            return None;
        }

        let target = self
            .route_options
            .get(self.route_combo.selected() as usize)?
            .target
            .clone();

        Some(RoutingRule {
            display_name,
            match_app_names,
            match_binaries,
            target,
            enabled: true,
        })
    }

    fn provisional_rule(&self) -> Option<RoutingRule> {
        let selected = self.selected_streams();
        let names = distinct(selected.iter().filter_map(|s| s.app_name.clone()));
        let bins = distinct(selected.iter().filter_map(|s| s.binary.clone()));
        self.rule_from(&names, &bins)
    }

    fn set_checked(&self, idx: usize, on: bool) {
        self.checked[idx].set(on);
        let w = &self.rows[idx];
        if on {
            w.row.add_css_class("stream-row-selected");
            w.check.add_css_class("checked");
            w.check.set_icon_name(Some("object-select-symbolic"));
        } else {
            w.row.remove_css_class("stream-row-selected");
            w.check.remove_css_class("checked");
            w.check.set_icon_name(None);
        }
        w.revealer.set_reveal_child(on);
    }

    fn recompute(&self) {
        let selected = self.selected_streams();
        let names = distinct(selected.iter().filter_map(|s| s.app_name.clone()));
        let bins = distinct(selected.iter().filter_map(|s| s.binary.clone()));

        if selected.is_empty() {
            self.match_group
                .set_description(Some("Pick a stream above to build the rule"));
            self.match_list.set_visible(false);
            self.warning.set_visible(false);
        } else if !shared_identity(&names, &bins) {
            self.match_group.set_description(None);
            self.match_list.set_visible(false);
            self.warning.set_visible(true);
            self.warning_caption
                .set_text(&no_identity_message(&self.selected_titles()));
        } else {
            self.match_group
                .set_description(Some(&self.sentence(&names, &bins)));
            self.match_list.set_visible(true);
            self.warning.set_visible(false);
            self.configure_field(&self.name_row, &self.name_switch, &names, "name");
            self.configure_field(&self.bin_row, &self.bin_switch, &bins, "binary");
        }

        let rule = self.rule_from(&names, &bins);
        self.refresh_preview(rule.as_ref());
        self.add_button.set_sensitive(rule.is_some());
    }

    fn refresh_suggestion(&self) {
        if self.user_edited.get() {
            return;
        }
        let suggested = suggested_display_name(&self.selected_streams());
        self.suggesting.set(true);
        self.name_entry.set_text(&suggested);
        self.suggesting.set(false);
    }

    fn selected_titles(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (g, checked) in self.groups.iter().zip(&self.checked) {
            if checked.get() && !out.contains(&g.title) {
                out.push(g.title.clone());
            }
        }
        out
    }

    fn configure_field(
        &self,
        row: &adw::ActionRow,
        switch: &gtk::Switch,
        values: &[String],
        field: &str,
    ) {
        let available = !values.is_empty();
        row.set_visible(available);
        if !available {
            return;
        }
        let subtitle = if switch.is_active() {
            field_summary(values)
        } else {
            format!("Off — matches any {field}")
        };
        row.set_subtitle(&subtitle);
    }

    fn sentence(&self, names: &[String], bins: &[String]) -> String {
        let mut clauses: Vec<String> = Vec::new();
        if self.name_switch.is_active()
            && let Some(c) = clause("name", names)
        {
            clauses.push(c);
        }
        if self.bin_switch.is_active()
            && let Some(c) = clause("binary", bins)
        {
            clauses.push(c);
        }
        if clauses.is_empty() {
            "Turn on a property below to match anything".to_owned()
        } else {
            format!("Matches every stream where {}", clauses.join(" and "))
        }
    }

    fn refresh_preview(&self, rule: Option<&RoutingRule>) {
        while let Some(child) = self.preview_list.first_child() {
            self.preview_list.remove(&child);
        }

        let mut matched = 0usize;
        for info in &self.streams {
            let hit = rule.is_some_and(|r| r.matches(info));
            if hit {
                matched += 1;
            }
            self.preview_list.append(&preview_row(info, hit));
        }

        let total = self.streams.len();
        self.preview_group.set_description(Some(&format!(
            "{matched} of {total} streams playing right now match this rule"
        )));
    }
}

pub fn show(
    transient_for: Option<&impl IsA<gtk::Window>>,
    mut streams: Vec<StreamInfo>,
    hw_sinks: Vec<HwSink>,
    preselect: &[u32],
    backend: PipeWireBackend,
    on_saved: impl Fn(RoutingRule) + 'static,
) -> adw::Window {
    streams.sort_by_key(stream_sort_key);
    let groups = build_groups(&streams);

    let win = adw::Window::builder()
        .title("Add Routing Rule")
        .default_width(560)
        .default_height(720)
        .modal(true)
        .build();
    if let Some(parent) = transient_for {
        win.set_transient_for(Some(parent));
    }

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    toolbar.add_top_bar(&header);

    let cancel = gtk::Button::with_label("Cancel");
    cancel.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| win.close()
    ));
    header.pack_start(&cancel);

    let add_button = gtk::Button::with_label("Add");
    add_button.add_css_class("suggested-action");
    add_button.set_sensitive(false);
    header.pack_end(&add_button);
    win.set_default_widget(Some(&add_button));

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let clamp = adw::Clamp::builder().maximum_size(560).build();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(12)
        .margin_end(12)
        .build();

    // the multi-select stream picker
    let stream_group = adw::PreferencesGroup::new();
    stream_group.set_title("Apps &amp; streams");

    let stream_stack = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    stream_group.add(&stream_stack);
    body.append(&stream_group);

    // the matched properties from the selected stream
    let match_group = adw::PreferencesGroup::new();
    match_group.set_title("Match on");

    let match_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    match_list.add_css_class("boxed-list");

    let name_switch = gtk::Switch::builder()
        .active(true)
        .valign(gtk::Align::Center)
        .build();
    let name_row = adw::ActionRow::new();
    name_row.set_title("Application name");
    name_row.set_tooltip_text(Some("application.name"));
    name_row.add_suffix(&name_switch);
    name_row.set_activatable_widget(Some(&name_switch));
    match_list.append(&name_row);

    let bin_switch = gtk::Switch::builder()
        .active(true)
        .valign(gtk::Align::Center)
        .build();
    let bin_row = adw::ActionRow::new();
    bin_row.set_title("Process binary");
    bin_row.set_tooltip_text(Some("application.process.binary"));
    bin_row.add_suffix(&bin_switch);
    bin_row.set_activatable_widget(Some(&bin_switch));
    match_list.append(&bin_row);

    match_group.add(&match_list);

    let (warning, warning_caption) = build_warning();
    match_group.add(&warning);
    body.append(&match_group);

    // selected output for the rule
    let route_group = adw::PreferencesGroup::new();
    route_group.set_title("Route to");
    let route_options = route_targets(&hw_sinks);
    let route_labels: Vec<&str> = route_options.iter().map(|o| o.label.as_str()).collect();
    let route_model = gtk::StringList::new(&route_labels);
    let route_combo = adw::ComboRow::builder()
        .title("Output device")
        .model(&route_model)
        .build();
    route_group.add(&route_combo);
    body.append(&route_group);

    // rule name
    let name_group = adw::PreferencesGroup::new();
    name_group.set_title("Display name");
    let name_entry = adw::EntryRow::new();
    name_entry.set_title("Name this rule");
    name_group.add(&name_entry);
    body.append(&name_group);

    // the preview of streams that match the rule
    let preview_group = adw::PreferencesGroup::new();
    preview_group.set_title("Preview");
    let preview_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    preview_list.add_css_class("boxed-list");
    preview_group.add(&preview_list);
    body.append(&preview_group);

    clamp.set_child(Some(&body));
    scroll.set_child(Some(&clamp));
    toolbar.set_content(Some(&scroll));
    win.set_content(Some(&toolbar));

    let mut rows = Vec::with_capacity(groups.len());
    let mut checked = Vec::with_capacity(groups.len());
    let mut cards: Vec<gtk::ListBox> = Vec::with_capacity(groups.len());
    let mut meters: Vec<(gtk::LevelBar, Vec<u32>)> = Vec::with_capacity(groups.len());
    if groups.is_empty() {
        let empty = adw::ActionRow::new();
        empty.set_title("No streams playing right now");
        empty.set_subtitle("Start audio in an app to route it");
        empty.set_activatable(false);
        let card = stream_card();
        card.append(&empty);
        stream_stack.append(&card);
    } else {
        for group in &groups {
            let (row, widgets, meter) = build_group_row(group);
            let card = stream_card();
            card.append(&row);
            stream_stack.append(&card);
            cards.push(card);
            let ids = group
                .stream_idx
                .iter()
                .map(|&i| streams[i].node_id)
                .collect();
            meters.push((meter, ids));
            rows.push(widgets);
            checked.push(Cell::new(false));
        }
    }

    let ui = Rc::new(Ui {
        streams,
        groups,
        checked,
        rows,
        name_switch,
        bin_switch,
        name_row,
        bin_row,
        match_list,
        match_group,
        warning,
        warning_caption,
        route_options,
        route_combo,
        name_entry,
        user_edited: Cell::new(false),
        suggesting: Cell::new(false),
        preview_group,
        preview_list,
        add_button: add_button.clone(),
    });

    for (idx, card) in cards.into_iter().enumerate() {
        card.connect_row_activated(glib::clone!(
            #[weak]
            ui,
            move |_, _| {
                let now = !ui.checked[idx].get();
                ui.set_checked(idx, now);
                ui.refresh_suggestion();
                ui.recompute();
            }
        ));
    }

    ui.name_switch.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        move |_| ui.recompute()
    ));
    ui.bin_switch.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        move |_| ui.recompute()
    ));
    ui.route_combo.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        move |_| ui.recompute()
    ));

    ui.name_entry.connect_changed(glib::clone!(
        #[weak]
        ui,
        move |entry| {
            if ui.suggesting.get() {
                return;
            }
            // clearing the entry hands naming back to the suggestions
            ui.user_edited.set(!entry.text().is_empty());
            ui.recompute();
        }
    ));

    {
        let ui_weak = Rc::downgrade(&ui);
        let on_saved = Rc::new(on_saved);
        add_button.connect_clicked(glib::clone!(
            #[weak]
            win,
            move |_| {
                let Some(ui) = ui_weak.upgrade() else { return };
                if let Some(rule) = ui.provisional_rule() {
                    on_saved(rule);
                    win.close();
                }
            }
        ));
    }

    // the dialog pauses the routing tile's meters
    let tick_source: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    if !meters.is_empty() {
        let id = glib::timeout_add_local(Duration::from_millis(40), move || {
            drive_stream_meters(&backend, &meters);
            glib::ControlFlow::Continue
        });
        tick_source.set(Some(id));
    }

    // the windows owns the dialog state for its lifetime
    win.connect_close_request(glib::clone!(
        #[strong]
        ui,
        #[strong]
        tick_source,
        move |_| {
            let _keep = &ui;
            if let Some(id) = tick_source.take() {
                id.remove();
            }
            glib::Propagation::Proceed
        }
    ));

    for (idx, group) in ui.groups.iter().enumerate() {
        let hit = group
            .stream_idx
            .iter()
            .any(|&i| preselect.contains(&ui.streams[i].node_id));
        if hit {
            ui.set_checked(idx, true);
        }
    }
    ui.refresh_suggestion();
    ui.recompute();

    win.present();
    win
}

fn build_groups(streams: &[StreamInfo]) -> Vec<Group> {
    let mut order: Vec<(Option<String>, Option<String>)> = Vec::new();
    let mut buckets: Vec<Vec<usize>> = Vec::new();

    for (i, s) in streams.iter().enumerate() {
        let key = (s.app_name.clone(), s.binary.clone());
        match order.iter().position(|k| k == &key) {
            Some(pos) => buckets[pos].push(i),
            None => {
                order.push(key);
                buckets.push(vec![i]);
            }
        }
    }

    let mut groups: Vec<Group> = order
        .into_iter()
        .zip(buckets)
        .map(|((app_name, binary), stream_idx)| {
            let title = group_title(&app_name, &binary);
            let icon = resolve_app_icon(streams[stream_idx[0]].app_icon.as_deref());
            let media_names = stream_idx
                .iter()
                .map(|&i| {
                    streams[i]
                        .media_name
                        .clone()
                        .unwrap_or_else(|| "Untitled stream".to_owned())
                })
                .collect();
            Group {
                title,
                binary,
                icon,
                stream_idx,
                media_names,
            }
        })
        .collect();

    groups.sort_by_key(|g| g.title.to_lowercase());
    groups
}

fn stream_card() -> gtk::ListBox {
    let card = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    card.add_css_class("boxed-list");
    card
}

fn build_group_row(group: &Group) -> (gtk::ListBoxRow, RowWidgets, gtk::LevelBar) {
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();

    let icon = gtk::Image::from_icon_name(&group.icon);
    icon.set_pixel_size(28);
    head.append(&icon);

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();

    let name = gtk::Label::builder()
        .label(&group.title)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    text.append(&name);

    let sub = gtk::Label::builder()
        .label(group_subtitle(group))
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    sub.add_css_class("dim-label");
    sub.add_css_class("caption");
    text.append(&sub);
    head.append(&text);

    let meter = row_level_meter();
    head.append(&meter);

    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .build();

    let check = gtk::Image::new();
    check.add_css_class("stream-check");
    check.set_valign(gtk::Align::Center);
    head.append(&check);

    outer.append(&head);

    // individual streams behind the group
    let children = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    children.add_css_class("stream-substreams");
    for media in &group.media_names {
        let line = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build();

        let bullet = gtk::Label::new(Some("–"));
        bullet.add_css_class("dim-label");
        bullet.set_width_request(52);
        bullet.set_valign(gtk::Align::Center);
        line.append(&bullet);

        let label = gtk::Label::builder()
            .label(media)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        label.add_css_class("dim-label");
        label.add_css_class("caption");
        line.append(&label);

        children.append(&line);
    }
    revealer.set_child(Some(&children));
    outer.append(&revealer);

    let row = gtk::ListBoxRow::builder()
        .activatable(true)
        .child(&outer)
        .build();

    (
        row.clone(),
        RowWidgets {
            row,
            check,
            revealer,
        },
        meter,
    )
}

fn preview_row(info: &StreamInfo, matched: bool) -> adw::ActionRow {
    let title = info
        .app_name
        .clone()
        .or_else(|| info.binary.clone())
        .unwrap_or_default();

    let row = adw::ActionRow::new();
    row.set_title(&esc(&title));
    row.set_subtitle(&preview_subtitle(info));
    row.set_title_lines(1);
    row.set_subtitle_lines(1);
    row.set_activatable(false);

    let tag = if matched {
        preview_tag("Matched", "match-tag")
    } else {
        row.add_css_class("preview-row-skipped");
        preview_tag("Skipped", "skipped-tag")
    };
    row.add_suffix(&tag);

    row
}

fn preview_tag(label: &str, class: &str) -> gtk::Label {
    let tag = gtk::Label::new(Some(label));
    tag.add_css_class(class);
    tag.set_valign(gtk::Align::Center);
    tag
}

fn build_warning() -> (gtk::Box, gtk::Label) {
    let banner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .visible(false)
        .build();
    banner.add_css_class("identity-warning");

    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.set_valign(gtk::Align::Start);
    banner.append(&icon);

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    let heading = gtk::Label::builder()
        .label("No shared identity")
        .xalign(0.0)
        .build();
    heading.add_css_class("heading");
    text.append(&heading);

    let caption = gtk::Label::builder().xalign(0.0).wrap(true).build();
    caption.add_css_class("caption");
    text.append(&caption);
    banner.append(&text);

    (banner, caption)
}

// escape problematic characters before getting to the markup-parsing
fn esc(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

fn field_summary(values: &[String]) -> String {
    match values {
        [] => String::new(),
        [one] => esc(one),
        many => many.iter().map(|s| esc(s)).collect::<Vec<_>>().join(", "),
    }
}

fn clause(field: &str, values: &[String]) -> Option<String> {
    match values {
        [] => None,
        [one] => Some(format!("{field} is {}", esc(one))),
        many => Some(format!("{field} is one of {}", many.len())),
    }
}

// The selection must match on at least one property
fn shared_identity(names: &[String], bins: &[String]) -> bool {
    names.len() == 1 || bins.len() == 1
}

fn no_identity_message(titles: &[String]) -> String {
    let apps = match titles {
        [a, b] => format!("{a} and {b}"),
        _ => titles.join(", "),
    };
    format!(
        "{apps} share neither a name nor a binary. A rule targets one app identity. Add them as separate rules."
    )
}

fn group_title(app_name: &Option<String>, binary: &Option<String>) -> String {
    app_name
        .clone()
        .or_else(|| binary.as_deref().map(strip_binary_suffix))
        .unwrap_or_else(|| "Unknown app".to_owned())
}

fn group_subtitle(group: &Group) -> String {
    let count = stream_count(group.stream_idx.len());
    match &group.binary {
        Some(bin) => format!("{bin} · {count}"),
        None => count,
    }
}

fn preview_subtitle(info: &StreamInfo) -> String {
    [&info.binary, &info.media_name]
        .into_iter()
        .filter_map(|v| v.as_deref())
        .map(esc)
        .collect::<Vec<_>>()
        .join(" · ")
}

fn stream_sort_key(info: &StreamInfo) -> (String, String, String) {
    (
        info.app_name.clone().unwrap_or_default().to_lowercase(),
        info.binary.clone().unwrap_or_default(),
        info.media_name.clone().unwrap_or_default(),
    )
}

fn distinct(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

fn resolve_app_icon(name: Option<&str>) -> String {
    let Some(display) = gtk::gdk::Display::default() else {
        return "audio-x-generic-symbolic".to_owned();
    };
    let theme = gtk::IconTheme::for_display(&display);
    name.filter(|n| theme.has_icon(n))
        .map(str::to_owned)
        .unwrap_or_else(|| "audio-x-generic-symbolic".to_owned())
}

/// Strips suffix from binary and title case
fn strip_binary_suffix(binary: &str) -> String {
    let stem = binary
        .strip_suffix(".bin")
        .or_else(|| binary.strip_suffix(".exe"))
        .or_else(|| binary.strip_suffix(".app"))
        .unwrap_or(binary);
    let mut chars = stem.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => stem.to_owned(),
    }
}

/// Smart default name for the selection, prefer the app name
fn suggested_display_name(selected: &[&StreamInfo]) -> String {
    match selected {
        [] => String::new(),
        [one] => match (&one.app_name, &one.binary) {
            (Some(name), _) => name.clone(),
            (None, Some(bin)) => strip_binary_suffix(bin),
            (None, None) => String::new(),
        },
        many => {
            let mut shared: Option<&str> = None;
            for c in many {
                let Some(n) = c.app_name.as_deref() else {
                    return String::new();
                };
                match shared {
                    None => shared = Some(n),
                    Some(s) if s == n => {}
                    Some(_) => return String::new(),
                }
            }
            shared.map(str::to_owned).unwrap_or_default()
        }
    }
}
