// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! A small effect's editor: painting and hit regions (platform independent). The Lemon "Pith" plugin look, as Lemon
//! Delay's, laid out as the original's editor: outlined cards under ruled headings holding knobs where the original
//! has knobs, faders where it has sliders and radio lists where it has radio buttons, a well that pictures the
//! settings, the hint well and the wordmark.
//!
// spec: plugins/lemon-small-effects.md (§2 the frame, §3 hints), the plugin pages (plugins/lemon-soft-clipper.md,
// lemon-fast-distortion.md, lemon-balance.md, lemon-stereo-enhancer.md, lemon-transient-processor.md, lemon-chorus.md,
// lemon-stereo-shaper.md; each §2 controls); ui/lemon-design.md §11.1

use os_theme::Palette;
use os_theme::design::lemon as brand;
use os_ui::kit::eq as keq;
use os_ui::kit::lemon::{self, plug};
use os_ui::kit::{State, inst};
use os_ui::{Canvas, Color, Font, Rect};

use crate::{Control, MAX, Mark, PanelKind, PanelLane, Spec};

/// Dials bigger than this get the thicker arc.
const SMALL: f32 = 19.0;

const RULE: Color = Color::rgb(0x3c3c3c);
const HEADING: Font = Font::condensed(11, 0).bold();
const LABEL: Font = Font::new(10).bold();
const HINT: Font = Font::new(11);
const VALUE: Font = Font::new(11).bold();
const MARK_BIG: Font = Font::condensed(22, 0).bold();
const MARK_SMALL: Font = Font::condensed(10, 0).bold();

/// Read access to parameter values by id.
pub trait Model {
    fn norm(&self, id: &str) -> f32;
    fn plain(&self, id: &str) -> f32;
    fn text(&self, id: &str) -> String;
}

impl Model for os_plugkit::vals::Vals {
    fn norm(&self, id: &str) -> f32 {
        os_plugkit::vals::Vals::norm(self, id)
    }
    fn plain(&self, id: &str) -> f32 {
        os_plugkit::vals::Vals::plain(self, id)
    }
    fn text(&self, id: &str) -> String {
        os_plugkit::vals::Vals::text(self, id)
    }
}

/// The values the editor shows: the parameters, with the spec's shown values over them (Lime 12's out knob shows
/// the effective output gain while the gains are linked).
pub struct Shown<'a> {
    pub spec: &'static Spec,
    pub vals: &'a os_plugkit::vals::Vals,
}

impl Model for Shown<'_> {
    fn norm(&self, id: &str) -> f32 {
        shown_norm(self.spec, self.vals, id)
    }
    fn plain(&self, id: &str) -> f32 {
        shown_plain(self.spec, self.vals, id)
    }
    fn text(&self, id: &str) -> String {
        shown_text(self.spec, self.vals, id)
    }
}

/// A setting's shown offset right now (`None` = it shows its stored value): the spec's mapping over the stored
/// values.
pub fn offset(spec: &Spec, vals: &os_plugkit::vals::Vals, k: usize) -> Option<i32> {
    let f = spec.shown?;
    let v = values(spec, vals);
    f(&v[..spec.settings.len()], k)
}

/// A setting's shown plain value: its stored one less the spec's offset, in its range.
pub fn shown_plain(spec: &Spec, vals: &os_plugkit::vals::Vals, id: &str) -> f32 {
    let stored = vals.plain(id);
    let Some(k) = spec.index(id) else {
        return stored;
    };
    let Some(off) = offset(spec, vals, k) else {
        return stored;
    };
    let (lo, hi) = spec.settings[k].range;
    (stored - off as f32).clamp(lo as f32, hi as f32)
}

/// A setting's shown normalized value.
pub fn shown_norm(spec: &Spec, vals: &os_plugkit::vals::Vals, id: &str) -> f32 {
    match spec.index(id).and_then(|k| offset(spec, vals, k)) {
        None => vals.norm(id),
        Some(_) => vals.plain_to_norm(id, shown_plain(spec, vals, id)),
    }
}

/// The stored plain that shows as `shown`: the shown value plus the spec's offset, in the range.
pub fn stored_plain(spec: &Spec, vals: &os_plugkit::vals::Vals, id: &str, shown: f32) -> f32 {
    let Some(k) = spec.index(id) else {
        return shown;
    };
    let Some(off) = offset(spec, vals, k) else {
        return shown;
    };
    let (lo, hi) = spec.settings[k].range;
    (shown + off as f32).clamp(lo as f32, hi as f32)
}

/// A setting's shown value text.
pub fn shown_text(spec: &Spec, vals: &os_plugkit::vals::Vals, id: &str) -> String {
    match spec.index(id) {
        Some(k) if offset(spec, vals, k).is_some() => {
            (spec.text)(k, shown_plain(spec, vals, id).round() as i32)
        }
        _ => vals.text(id),
    }
}

/// What a region of the editor does when clicked or dragged.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Vertical drag / wheel; menu on right click.
    Knob(String),
    /// As a knob, the drag over the fader's travel (px).
    Fader(String, f32),
    /// One position of a selector (click picks it).
    Choice(String, i32),
    /// A text field (a click types in its text).
    Text(String),
    /// The button that opens a lane's controls in the panel.
    Lane(usize),
    /// The wordmark (the About menu).
    Logo,
}

impl Target {
    /// The setting's id ("" for the wordmark).
    pub fn id(&self) -> &str {
        match self {
            Target::Knob(id) | Target::Fader(id, _) | Target::Choice(id, _) | Target::Text(id) => {
                id
            }
            Target::Lane(_) | Target::Logo => "",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub r: Rect,
    pub t: Target,
}

/// The pointer and the pressed control.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub mouse: Option<(f32, f32)>,
    pub pressed: Option<Target>,
    pub playhead: Option<usize>,
    /// The lane whose controls the panel shows.
    pub selected: usize,
    /// The saved state the text fields show ([`Control::Text`]).
    pub raw: Vec<u8>,
}

struct Ui<'a> {
    c: &'a mut Canvas,
    t: &'a Palette,
    m: &'a dyn Model,
    st: &'a ViewState,
    hits: Vec<Hit>,
    accent: Color,
}

/// A lane's hue: the rows spread over the spectrum from red to magenta, 30 degrees apart (11 lanes).
fn lane_color(row: usize) -> Color {
    let h = (row % 11) as f32 * 30.0 / 60.0;
    let (s, v) = (0.83f32, 1.0f32);
    let f = h - h.floor();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match h.floor() as u32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let byte = |x: f32| (x * 255.0).round() as u32;
    let hue = Color::rgb(byte(r) << 16 | byte(g) << 8 | byte(b));
    // Pure blue and violet read dark on the grey background.
    if (8..=9).contains(&(row % 11)) { hue.mix(Color::rgb(0xffffff), 0.3) } else { hue }
}

/// A lane's small button: a translucent fill and a bright outline in the lane's hue; `on` brightens the fill.
fn lane_button(c: &mut Canvas, r: Rect, hue: Color, on: bool, hover: bool) {
    let fill = if on { 110 } else if hover { 60 } else { 28 };
    c.fill_rrect(r, 3.0, hue.with_alpha(fill));
    c.stroke_rrect(r.inset(1.0), 2.0, 2.0, hue.with_alpha(if on { 255 } else { 150 }));
}

impl Ui<'_> {
    fn hit(&mut self, r: Rect, t: Target) {
        self.hits.push(Hit { r, t });
    }
    fn state(&self, r: Rect, t: &Target) -> State {
        let pressed = self.st.pressed.as_ref() == Some(t);
        let hover = !pressed
            && self.st.pressed.is_none()
            && self.st.mouse.is_some_and(|(x, y)| r.contains(x, y));
        State {
            hover,
            pressed,
            ..State::NORMAL
        }
    }
}

/// The settings shown (the first `n` of `MAX`).
pub fn values(spec: &Spec, m: &dyn Model) -> [i32; MAX] {
    std::array::from_fn(|k| {
        spec.settings
            .get(k)
            .map_or(0, |s| m.plain(s.id).round() as i32)
    })
}

/// Paints the editor; `hint` (empty when the host shows it) goes in the hint well, or over the display's top-left
/// corner.
pub fn paint(
    spec: &Spec,
    c: &mut Canvas,
    t: &Palette,
    m: &dyn Model,
    st: &ViewState,
    hint: &str,
) -> Vec<Hit> {
    let mut ui = Ui {
        c,
        t,
        m,
        st,
        hits: Vec::new(),
        accent: brand::MAIN,
    };
    ui.c.begin_frame(None);
    ui.c.clear(plug::BG);
    if spec.controls.iter().any(|c| matches!(c, Control::StepGrid { .. })) {
        ui.accent = lane_color(ui.st.selected);
    }
    for card in spec.cards {
        lemon::plug_panel(ui.c, Rect::span(card.x0, card.x1, card.y0, card.y1));
        if !card.heading.is_empty() {
            heading(&mut ui, card.x0, card.x1, card.y0 - 12.0, card.heading);
        }
    }
    for control in spec.controls {
        match *control {
            Control::Knob {
                id,
                cx,
                cy,
                r,
                caption,
                bipolar,
            } => knob(&mut ui, id, cx, cy, r, caption, bipolar),
            Control::Choice { id, x, top, names } => {
                let min = spec.index(id).map_or(0, |k| spec.settings[k].range.0);
                choices(&mut ui, id, x, top, min, names);
            }
            Control::Fader {
                id,
                x,
                top,
                bot,
                label,
                col,
                bipolar,
                marks,
            } => fader(&mut ui, id, x, top, bot, label, col, bipolar, marks),
            Control::Shape { id, cx, cy } => {
                let range = spec.index(id).map_or((0, 1), |k| spec.settings[k].range);
                shape(&mut ui, id, cx, cy, range);
            }
            Control::Switch { id, cx, cy } => {
                let range = spec.index(id).map_or((0, 1), |k| spec.settings[k].range);
                switch_box(&mut ui, id, cx, cy, range);
            }
            Control::StepGrid {
                ids,
                labels,
                enabled_ids,
                columns,
                rows,
                length_id,
                rect,
            } => {
                step_grid(
                    &mut ui,
                    ids,
                    labels,
                    enabled_ids,
                    columns,
                    rows,
                    length_id,
                    rect,
                );
            }
            Control::Panel { lanes, rect } => panel(&mut ui, spec, lanes, rect),
            Control::Tile {
                id,
                x0,
                y0,
                x1,
                y1,
                caption,
            } => tile(&mut ui, id, Rect::span(x0, x1, y0, y1), caption),
            Control::Text {
                id,
                x0,
                y0,
                x1,
                y1,
                show,
                ..
            } => {
                let (text, status) = show(&st.raw);
                text_field(&mut ui, id, Rect::span(x0, x1, y0, y1), &text, &status);
            }
        }
    }
    if let Some(d) = &spec.display {
        lemon::plug_well(ui.c, d.r, 6.0);
        let v = values(spec, m);
        (d.draw)(ui.c, d.r, &v[..spec.settings.len()]);
    }
    for &(cx, top, s) in spec.labels {
        inst::caption(ui.c, ui.t, cx, top, s, false);
    }
    let mark = logo(&mut ui, spec);
    let lh = ui.c.line_height(HINT);
    match (spec.hint, &spec.display) {
        (Some(mut well), _) => {
            if well.y < mark.bottom() && mark.y < well.bottom() && well.right() > mark.x - 16.0 {
                well = Rect::span(well.x, mark.x - 16.0, well.y, well.bottom());
            }
            lemon::plug_well(ui.c, well, 6.0);
            if !hint.is_empty() {
                ui.c.text_clip(
                    HINT,
                    well.x + 8.0,
                    (well.cy() - lh / 2.0).round(),
                    well.w - 16.0,
                    brand::TEXT,
                    hint,
                );
            }
        }
        (None, Some(d)) if !hint.is_empty() => {
            let w = (ui.c.text_width(HINT, hint) + 12.0).min(d.r.w - 8.0);
            let r = Rect::new(d.r.x + 4.0, d.r.y + 4.0, w, lh + 6.0);
            ui.c.fill_rrect(r, 4.0, plug::WELL.with_alpha(220));
            ui.c.text_clip(HINT, r.x + 6.0, r.y + 3.0, r.w - 12.0, brand::TEXT, hint);
        }
        _ => {}
    }
    ui.hits
}

/// A card's heading (as Lemon Delay's): letter-spaced caps centred on a hairline rule at `y`, the rule stopping 7 px
/// short of the text (none where the card is too narrow for it).
fn heading(ui: &mut Ui, x0: f32, x1: f32, y: f32, s: &str) {
    let lh = ui.c.line_height(HEADING);
    let w = lemon::spaced_width(ui.c, HEADING, s, 2.0);
    let tx = ((x0 + x1 + 1.0) / 2.0 - w / 2.0).round();
    lemon::spaced(ui.c, HEADING, tx, (y - lh / 2.0).round(), plug::DIM, s, 2.0);
    if tx - 7.0 > x0 + 8.0 {
        ui.c.fill_rect(Rect::span(x0 + 4.0, tx - 7.0, y, y + 1.0), RULE);
        ui.c.fill_rect(
            Rect::span((tx + w + 6.0).round(), x1 - 4.0, y, y + 1.0),
            RULE,
        );
    }
}

/// A pith dial of radius `r` (hit area 3 px around it), caption (or the value text) under it.
fn knob(ui: &mut Ui, id: &str, cx: f32, cy: f32, r: f32, cap: &str, bipolar: bool) {
    let target = Target::Knob(id.into());
    let hr = Rect::new(cx - r - 3.0, cy - r - 3.0, 2.0 * r + 6.0, 2.0 * r + 6.0);
    let st = ui.state(hr, &target);
    lemon::dial(
        ui.c,
        cx,
        cy,
        r,
        if r > SMALL { 3.2 } else { 2.8 },
        ui.m.norm(id),
        bipolar,
        ui.accent,
        st,
    );
    if cap.is_empty() {
        let lh = ui.c.line_height(LABEL);
        let text = ui.m.text(id);
        ui.c.text_center(
            LABEL,
            Rect::new(cx - 50.0, cy + r + 6.0, 100.0, lh),
            ui.accent,
            &text,
        );
    } else {
        inst::caption(ui.c, ui.t, cx, cy + r + 7.0, cap, false);
    }
    ui.hit(hr, target);
}

/// A radio list (as the original's): one row per position from `top` (the first is the range's minimum), a toggle
/// lit lemon on the chosen one and the name beside it, in lemon when chosen.
fn choices(ui: &mut Ui, id: &str, x: f32, top: f32, min: i32, names: &[&str]) {
    let chosen = ui.m.plain(id).round() as i32;
    let lh = ui.c.line_height(LABEL);
    for (i, name) in names.iter().enumerate() {
        let v = min + i as i32;
        let target = Target::Choice(id.into(), v);
        let y = top + 16.0 * i as f32;
        let w = ui.c.text_width(LABEL, name);
        let r = Rect::span(x - 2.0, (x + 20.0 + w).ceil(), y, y + 15.0);
        let st = ui.state(r, &target);
        if ui.accent != brand::MAIN && chosen == v {
            let dot = Rect::new(x, y + 1.5, 12.0, 12.0);
            ui.c.fill_rrect(dot, 4.0, ui.accent.with_alpha(64));
            ui.c.stroke_rrect(dot.inset(1.0), 3.0, 2.0, ui.accent);
        } else {
            lemon::toggle(ui.c, x + 6.0, y + 7.5, chosen == v, st);
        }
        let ink = if chosen == v {
            ui.accent
        } else if st.hover {
            brand::TEXT
        } else {
            plug::DIM
        };
        ui.c.text(LABEL, x + 17.0, (y + 7.5 - lh / 2.0).round(), ink, name);
        ui.hit(r, target);
    }
}

/// A fader (as Lemon Reverb's): a slot over `top..bot`, ticks at `marks`, a fill in `col` up to the handle (from the
/// middle when `bipolar`), `label` over it in `col`.
#[allow(clippy::too_many_arguments)]
fn fader(
    ui: &mut Ui,
    id: &str,
    x: f32,
    top: f32,
    bot: f32,
    label: &str,
    col: Color,
    bipolar: bool,
    marks: &[f32],
) {
    let target = Target::Fader(id.into(), bot - top);
    let hr = Rect::new(x - 11.0, top - 8.0, 22.0, bot - top + 16.0);
    let st = ui.state(hr, &target);
    keq::slot(
        ui.c,
        Rect::new(x - 3.0, top - 4.0, 6.0, bot - top + 8.0),
        st,
    );
    let at = |n: f32| bot - (bot - top) * n.clamp(0.0, 1.0);
    for &n in marks {
        let y = at(n).round();
        ui.c.fill_rect(Rect::new(x - 9.0, y, 3.0, 1.0), plug::FAINT);
        ui.c.fill_rect(Rect::new(x + 6.0, y, 3.0, 1.0), plug::FAINT);
    }
    let y = at(ui.m.norm(id));
    let from = if bipolar { at(0.5) } else { bot };
    if (from - y).abs() > 1.0 {
        ui.c.fill_rrect(
            Rect::span(x - 1.0, x + 1.0, y.min(from), y.max(from)),
            1.0,
            col.with_alpha(190),
        );
    }
    keq::handle(ui.c, x, y.round(), col, st);
    let lh = ui.c.line_height(LABEL);
    ui.c.text_center(
        LABEL,
        Rect::new(x - 12.0, top - 10.0 - lh, 24.0, lh),
        col,
        label,
    );
    ui.hit(hr, target);
}

/// An envelope shape button (as the original's): a raised button showing a rise, a hold and a fall, the rise sharper
/// for the lower values; a click steps to the next value (back to the first after the last).
fn shape(ui: &mut Ui, id: &str, cx: f32, cy: f32, (lo, hi): (i32, i32)) {
    let v = ui.m.plain(id).round() as i32;
    let next = if v >= hi || v < lo { lo } else { v + 1 };
    let target = Target::Choice(id.into(), next);
    let r = Rect::new(cx - 20.0, cy - 10.0, 40.0, 20.0);
    let st = ui.state(r, &target);
    lemon::plug_button(ui.c, r, 5.0, st);
    let n = if hi > lo {
        (v - lo).clamp(0, hi - lo) as f32 / (hi - lo) as f32
    } else {
        0.0
    };
    // The rise's curve: t^p, p from 0.35 (sharp) to 2.8 (slow); the fall mirrors it.
    let p = 0.35 * 8.0f32.powf(n);
    let (x0, x1, y0, y1) = (cx - 13.0, cx + 13.0, cy + 5.0, cy - 5.0);
    let mut pts = Vec::new();
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        pts.push((x0 + 9.0 * t, y0 + (y1 - y0) * t.powf(p)));
    }
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        pts.push((x1 - 9.0 + 9.0 * t, y1 + (y0 - y1) * t.powf(1.0 / p)));
    }
    ui.c.polyline(&pts, 1.5, brand::MAIN);
    ui.hit(r, target);
}

/// A small switch box (Lime 12's crush): a raised box lit mango on the range's top with the value's name inside;
/// a click steps between the two ends.
fn switch_box(ui: &mut Ui, id: &str, cx: f32, cy: f32, (lo, hi): (i32, i32)) {
    let v = ui.m.plain(id).round() as i32;
    let next = if v >= hi || v < lo { lo } else { hi };
    let target = Target::Choice(id.into(), next);
    let r = Rect::new(cx - 25.0, cy - 9.0, 50.0, 18.0);
    let st = State {
        on: v == hi && hi > lo,
        ..ui.state(r, &target)
    };
    let ink = lemon::plug_button(ui.c, r, 5.0, st);
    let text = ui.m.text(id).to_uppercase();
    let lh = ui.c.line_height(LABEL);
    ui.c.text_center(
        LABEL,
        Rect::new(r.x, (r.cy() - lh / 2.0).round(), r.w, lh),
        ink,
        &text,
    );
    ui.hit(r, target);
}

fn step_grid(
    ui: &mut Ui,
    ids: &[&[&str]],
    labels: &[&str],
    enabled_ids: &[&str],
    columns: usize,
    rows: usize,
    length_id: &str,
    rect: Rect,
) {
    let columns = columns.max(1);
    let rows = rows.max(1);
    let length = if ui.m.plain(length_id).round() <= 0.0 {
        columns / 2
    } else {
        columns
    };
    let cell_w = rect.w / columns as f32;
    let cell_h = rect.h / rows as f32;
    let lh = ui.c.line_height(LABEL);

    for row in 0..rows {
        let row_y = rect.y + row as f32 * cell_h;
        let selected = ui.st.selected == row;
        let target = Target::Lane(row);
        let r = Rect::new(rect.x - 184.0, row_y + 2.0, 18.0, (cell_h - 4.0).max(10.0));
        let hue = lane_color(row);
        let state = ui.state(r, &target);
        lane_button(ui.c, r, hue, selected, state.hover || state.pressed);
        ui.c.text_center(LABEL, Rect::new(r.x, r.cy() - lh / 2.0, r.w, lh), hue, ">");
        ui.hit(r, target);
        if let Some(label) = labels.get(row) {
            let w = inst::caption_width(ui.c, label, inst::CAPTION_TRACK);
            let x = (rect.x - 160.0 + 62.0 - w / 2.0).max(rect.x - 160.0);
            let ink = if selected { lane_color(row) } else { lane_color(row).with_alpha(170) };
            inst::caption_at(
                ui.c,
                x.round(),
                (row_y + 3.0).round(),
                ink,
                label,
                inst::CAPTION_TRACK,
            );
        }
        if let Some(&id) = enabled_ids.get(row) {
            let r = Rect::new(rect.x - 36.0, row_y + 2.0, 20.0, (cell_h - 4.0).max(10.0));
            let value = ui.m.plain(id).round() as i32;
            let next = if value > 0 { 0 } else { 10000 };
            let target = Target::Choice(id.to_string(), next);
            let state = ui.state(r, &target);
            lane_button(ui.c, r, lane_color(row), value > 0, state.hover || state.pressed);
            ui.hit(r, target);
        }
        let lane_rect = Rect::new(
            rect.x,
            row_y + 2.0,
            cell_w * length as f32,
            (cell_h - 4.0).max(1.0),
        );
        ui.c.fill_rrect(lane_rect, 3.0, plug::TRACK);
        if length < columns {
            ui.c.fill_rrect(
                Rect::new(
                    rect.x + cell_w * length as f32,
                    row_y + 2.0,
                    cell_w * (columns - length) as f32,
                    (cell_h - 4.0).max(1.0),
                ),
                3.0,
                plug::FACE.with_alpha(80),
            );
        }
        for column in 0..columns {
            let Some(&id) = ids.get(row).and_then(|lane| lane.get(column)) else {
                continue;
            };
            let r = Rect::new(
                rect.x + column as f32 * cell_w + 1.0,
                row_y + 1.0,
                (cell_w - 2.0).max(1.0),
                (cell_h - 2.0).max(1.0),
            );
            if column >= length {
                continue;
            }
            let value = ui.m.plain(id).round() as i32;
            let target = Target::Choice(id.into(), if value > 0 { 0 } else { 10000 });
            let state = ui.state(r, &target);
            if state.hover || state.pressed {
                ui.c.stroke_rrect(r.outset(-1.0), 2.0, 1.0, brand::TEXT.with_alpha(180));
            }
            ui.hit(r, target);
        }
        let mut column = 0;
        while column < length {
            if ui.m.plain(ids[row][column]).round() <= 0.0 {
                column += 1;
                continue;
            }
            let start = column;
            while column + 1 < length && ui.m.plain(ids[row][column + 1]).round() > 0.0 {
                column += 1;
            }
            let end = column;
            let clip = Rect::new(
                rect.x + start as f32 * cell_w + 1.0,
                row_y + 3.0,
                ((end - start + 1) as f32 * cell_w - 2.0).max(1.0),
                (cell_h - 6.0).max(1.0),
            );
            let hue = lane_color(row);
            ui.c.fill_rrect(clip, 3.0, hue.with_alpha(64));
            ui.c.stroke_rrect(clip.inset(1.0), 2.0, 2.0, hue);
            let grip_h = (clip.h - 6.0).max(4.0);
            for grip_x in [clip.x + 4.0, clip.right() - 7.0] {
                ui.c.fill_rrect(
                    Rect::new(grip_x, clip.cy() - grip_h / 2.0, 3.0, grip_h),
                    1.5,
                    hue.with_alpha(200),
                );
            }
            column += 1;
        }
    }

    for column in (4..columns).step_by(4) {
        let x = rect.x + column as f32 * cell_w;
        ui.c.fill_rect(Rect::new(x, rect.y, 1.0, rect.h), RULE.with_alpha(150));
    }
    if let Some(column) = ui.st.playhead.filter(|&column| column < length) {
        let x = rect.x + column as f32 * cell_w;
        ui.c.stroke_rrect(
            Rect::new(x + 0.5, rect.y + 0.5, cell_w - 1.0, rect.h - 1.0),
            2.0,
            1.5,
            brand::TEXT,
        );
    }
}

/// The controls of the selected lane: its radio lists in columns at the left, its dials after them, each dial with its
/// caption and value under it. The card's heading names the lane.
fn panel(ui: &mut Ui, spec: &Spec, lanes: &[PanelLane], rect: Rect) {
    let Some(lane) = lanes.get(ui.st.selected.min(lanes.len().saturating_sub(1))) else {
        return;
    };
    heading(
        ui,
        rect.x,
        rect.right(),
        rect.y - 12.0,
        &format!("{} CONTROLS", lane.label.to_uppercase()),
    );
    let mut x = rect.x + 16.0;
    ui.accent = lane_color(ui.st.selected.min(lanes.len().saturating_sub(1)));
    for item in lane.items {
        if let PanelKind::Choice { names } = item.kind {
            let min = spec.index(item.id).map_or(0, |k| spec.settings[k].range.0);
            let widest = names
                .iter()
                .map(|name| ui.c.text_width(LABEL, name))
                .fold(0.0, f32::max);
            inst::caption_at(
                ui.c,
                x.round(),
                (rect.y + 4.0).round(),
                plug::DIM,
                item.caption,
                inst::CAPTION_TRACK,
            );
            choices(ui, item.id, x, rect.y + 20.0, min, names);
            x += 20.0
                + widest.max(inst::caption_width(ui.c, item.caption, inst::CAPTION_TRACK) - 20.0)
                + 22.0;
        }
    }
    let mut cx = x + 40.0;
    for item in lane.items {
        if let PanelKind::Knob { bipolar } = item.kind {
            knob(ui, item.id, cx, rect.y + 38.0, 16.0, item.caption, bipolar);
            let lh = ui.c.line_height(LABEL);
            let text = ui.m.text(item.id);
            ui.c.text_center(
                LABEL,
                Rect::new(cx - 44.0, rect.y + 38.0 + 16.0 + 22.0, 88.0, lh),
                ui.accent,
                &text,
            );
            cx += 88.0;
        }
    }
}

/// A tile of a grid (Lemon Chorus's, as the original's): a card, a small dial at the left, the caption on a field at
/// the top right and the value text under it. The whole tile turns the dial.
fn tile(ui: &mut Ui, id: &str, r: Rect, caption: &str) {
    lemon::plug_panel(ui.c, r);
    let rad = ((r.h - 14.0) / 2.0).min(17.0);
    let (cx, cy) = (r.x + 8.0 + rad, r.cy());
    let target = Target::Knob(id.into());
    let hr = Rect::span(r.x + 2.0, r.right() - 2.0, r.y + 2.0, r.bottom() - 2.0);
    let st = ui.state(hr, &target);
    lemon::dial(
        ui.c,
        cx,
        cy,
        rad,
        2.8,
        ui.m.norm(id),
        false,
        brand::MAIN,
        st,
    );
    let x = cx + rad + 10.0;
    let band = Rect::span(x, r.right() - 7.0, r.y + 6.0, r.y + 21.0);
    lemon::field(ui.c, band, State::NORMAL);
    let lh = ui.c.line_height(LABEL);
    let w = lemon::spaced_width(ui.c, LABEL, caption, 1.0);
    lemon::spaced(
        ui.c,
        LABEL,
        (band.cx() - w / 2.0).round(),
        (band.cy() - lh / 2.0).round(),
        plug::DIM,
        caption,
        1.0,
    );
    let text = ui.m.text(id);
    let vh = ui.c.line_height(VALUE);
    ui.c.text_clip(
        VALUE,
        x + 4.0,
        (r.bottom() - 13.0 - vh / 2.0).round(),
        band.w - 4.0,
        brand::TEXT,
        &text,
    );
    ui.hit(hr, target);
}

/// A text field: the text in a field (clipped at its right end), the status under it in the caption's grey. A click
/// types the text in.
fn text_field(ui: &mut Ui, id: &str, r: Rect, text: &str, status: &str) {
    let target = Target::Text(id.into());
    let st = ui.state(r, &target);
    lemon::field(ui.c, r, st);
    let vh = ui.c.line_height(VALUE);
    ui.c.text_clip(
        VALUE,
        r.x + 8.0,
        (r.cy() - vh / 2.0).round(),
        r.w - 16.0,
        brand::TEXT,
        text,
    );
    ui.c.text_clip(
        LABEL,
        r.x + 2.0,
        r.bottom() + 7.0,
        r.w - 4.0,
        plug::DIM,
        status,
    );
    ui.hit(r, target);
}

/// The wordmark (as Lemon Delay's): "LIME" in mango beside the effect's name in large grey
/// capitals, or MANGO over the name (`stacked`), placed by the spec's [`Mark`]; returns where it is.
fn logo(ui: &mut Ui, spec: &Spec) -> Rect {
    let Mark {
        x,
        y,
        anchor,
        stacked,
    } = spec.mark;
    let ws = lemon::spaced_width(ui.c, MARK_SMALL, "LIME", 2.5);
    let (lh_b, lh_s) = (ui.c.line_height(MARK_BIG), ui.c.line_height(MARK_SMALL));
    if stacked {
        // MANGO centred over the name.
        let (wn, wl) = (lemon::spaced_width(ui.c, MARK_BIG, spec.word, 2.0), ws);
        let w = wn.max(wl);
        let left = (x - w * anchor).round();
        let lx = (left + (w - wl) / 2.0).round();
        lemon::spaced(ui.c, MARK_SMALL, lx, y, ui.accent, "LIME", 2.5);
        lemon::spaced(
            ui.c,
            MARK_BIG,
            (left + (w - wn) / 2.0).round(),
            y + lh_s - 2.0,
            plug::WORDMARK,
            spec.word,
            2.0,
        );
        let r = Rect::span(left - 2.0, left + w + 2.0, y - 2.0, y + lh_s + lh_b - 2.0);
        ui.hit(r, Target::Logo);
        return r;
    }
    let wb = lemon::spaced_width(ui.c, MARK_BIG, spec.word, 2.0);
    let total = ws + 8.0 + wb;
    let left = (x - total * anchor).round();
    let right = left + total;
    let bx = (right - wb).round();
    lemon::spaced(
        ui.c,
        MARK_BIG,
        bx,
        (y - lh_b / 2.0).round(),
        plug::WORDMARK,
        spec.word,
        2.0,
    );
    let sx = (bx - 8.0 - ws).round();
    lemon::spaced(
        ui.c,
        MARK_SMALL,
        sx,
        (y - lh_s / 2.0).round() + 1.0,
        ui.accent,
        "LIME",
        2.5,
    );
    let r = Rect::span(left - 2.0, right, y - 12.0, y + 12.0);
    ui.hit(r, Target::Logo);
    r
}

// ---------------------------------------------------------------------------------------------------------------
// Pieces the displays share

/// A transfer curve in a well: `f` maps input to output; the input runs over `-x_max..x_max` across, the output over
/// `-y_max..y_max` up (clipped to the well). Axes through the centre, and dim lines at full scale (±1 out).
pub fn transfer(c: &mut Canvas, r: Rect, x_max: f32, y_max: f32, f: impl Fn(f32) -> f32) {
    let (cx, cy) = (r.cx(), r.cy());
    let (hw, hh) = (r.w / 2.0 - 6.0, r.h / 2.0 - 6.0);
    c.fill_rect(
        Rect::new(r.x + 6.0, cy.round(), r.w - 12.0, 1.0),
        plug::GRID,
    );
    c.fill_rect(
        Rect::new(cx.round(), r.y + 6.0, 1.0, r.h - 12.0),
        plug::GRID,
    );
    for s in [-1.0f32, 1.0] {
        let y = (cy - s / y_max * hh).round();
        if (y - cy).abs() < hh {
            c.fill_rect(Rect::new(r.x + 6.0, y, r.w - 12.0, 1.0), plug::GRID);
        }
        let x = (cx + s / x_max * hw).round();
        if (x - cx).abs() < hw {
            c.fill_rect(Rect::new(x, r.y + 6.0, 1.0, r.h - 12.0), plug::GRID);
        }
    }
    let n = (r.w - 12.0).max(2.0) as usize;
    let pts: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
            let x = (i as f32 / n as f32 * 2.0 - 1.0) * x_max;
            let y = f(x);
            let y = if y.is_finite() {
                y.clamp(-y_max, y_max)
            } else {
                0.0
            };
            (cx + x / x_max * hw, cy - y / y_max * hh)
        })
        .collect();
    c.polyline(&pts, 1.5, brand::MAIN);
}

/// Two output meters side by side (L and R) for `left` and `right` as levels: a slim bar from the bottom of its slot
/// for each, full height at `top` (a level above it fills the bar; a negative one, an inverted output, is grey), a
/// tick at unity across both.
pub fn level_bars(c: &mut Canvas, r: Rect, left: f32, right: f32, top: f32) {
    const BAR: f32 = 14.0;
    // 26 px apart, closer in a narrow well.
    let gap = (r.w - 2.0 * BAR - 28.0).clamp(10.0, 26.0);
    let (y0, y1) = (r.y + 9.0, r.y + r.h - 22.0);
    let x0 = (r.cx() - BAR - gap / 2.0).round();
    let lh = c.line_height(LABEL);
    let unity = (y1 - (y1 - y0) / top).round();
    c.fill_rect(
        Rect::span(x0 - 8.0, x0 + 2.0 * BAR + gap + 8.0, unity, unity + 1.0),
        plug::TICK,
    );
    for (i, (v, name)) in [(left, "L"), (right, "R")].into_iter().enumerate() {
        let x = x0 + i as f32 * (BAR + gap);
        c.fill_rrect(Rect::span(x, x + BAR, y0, y1), 3.0, plug::TRACK);
        let h = ((y1 - y0) * (v.abs() / top).clamp(0.0, 1.0)).round();
        if h > 0.0 {
            let col = if v < 0.0 { plug::FAINT } else { brand::MAIN };
            c.fill_rrect(Rect::span(x, x + BAR, y1 - h, y1), 3.0, col);
        }
        c.text_center(
            LABEL,
            Rect::new(x - 8.0, y1 + 4.0, BAR + 16.0, lh),
            plug::DIM,
            name,
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Hints (spec §3)

/// A setting's hint name.
pub fn name(spec: &Spec, id: &str) -> String {
    spec.index(id)
        .map_or_else(|| id.to_string(), |k| spec.settings[k].name.to_string())
}

/// The hint line for a target.
pub fn hint(spec: &Spec, t: &Target, m: &dyn Model) -> String {
    match t {
        Target::Logo => spec.name.into(),
        Target::Lane(_) => String::new(),
        t => format!("{}: {}", name(spec, t.id()), m.text(t.id())),
    }
}
