// SPDX-License-Identifier: GPL-3.0-or-later

//! The scrolling sampler's strip (issue #82), docked between the toolbar and the playlist: the newest master output as
//! a scrolling wave; a selection on it drops onto the playlist as an audio clip. Audio ▸ Scrolling sampler turns the
//! sampler on and sets the tape's length; the arrow at the left collapses the strip.
//!
//! `App::tick` pulls the picture (`pull`), the paint draws it and registers the hits; the interactions live on
//! `App` (`Action::Scrolling`, `Drag::ScrollSel`).

use std::time::Instant;

use os_ui::Rect;

use super::FrameInfo;
use crate::app::{Action, App, State, Ui};
use crate::views::fnt;

/// The collapse arrow's column.
pub const ARROW_W: f32 = 18.0;
/// The strip's height with the wave shown.
const TALL: f32 = 52.0;
/// The strip's height collapsed.
const SHORT: f32 = 14.0;
/// The smallest view, in seconds, that Shift+wheel zooms into.
pub const MIN_ZOOM: f32 = 0.1;

/// What the strip's hits do.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// The wave: a press draws a selection; a press on one begins the drag out to the playlist.
    Wave,
    /// The arrow at the left: collapse or expand the strip.
    Collapse,
}

/// The strip's height in the window's px: none while the setting is off, one row collapsed, the wave tall.
pub fn strip_h(s: &State) -> f32 {
    if !crate::prefs::check(&crate::prefs::SCROLLING_SAMPLER) {
        0.0
    } else if s.scroll.collapsed {
        SHORT
    } else {
        TALL
    }
}

/// The strip's rect over a window `w` px wide: the band under the toolbar's, over the workspace's top.
pub fn rect(s: &State, w: f32) -> Rect {
    let h = strip_h(s);
    Rect::new(0.0, super::work_y(s) - h, w, h)
}

/// The seconds of tape the view shows; 0 means the whole tape.
pub fn shown(s: &State) -> f32 {
    let cap = crate::prefs::scrolling_secs();
    if s.scroll.zoom > 0.0 {
        s.scroll.zoom.min(cap.max(MIN_ZOOM))
    } else {
        cap
    }
}

/// The frame under `x` in the wave, clamped to the view's edges.
pub fn frame_at(s: &State, x: f32) -> u64 {
    let Some(w) = s.scroll.wave else { return s.scroll.from };
    let k = ((x - w.x) / w.w.max(1.0)).clamp(0.0, 1.0) as f64;
    s.scroll.from + (s.scroll.visible as f64 * k) as u64
}

/// The x of `frame` in the wave; frames off the view clamp to its edges.
pub fn x_of(s: &State, frame: u64) -> f32 {
    let Some(w) = s.scroll.wave else { return 0.0 };
    let k = (frame as f64 - s.scroll.from as f64) / s.scroll.visible.max(1) as f64;
    w.x + k.clamp(0.0, 1.0) as f32 * w.w
}

/// Pulls the strip's picture from the tape (in `App::tick`, and after every zoom, pan or collapse): the view's
/// frames, the newest frame heard, the wave's rect and the per-column peaks of the wave's width.
pub fn pull(app: &mut App) {
    let w = app.state.screen.0;
    let h = strip_h(&app.state);
    let (oldest, heard, rate) = app.transport.scrolling().window(Instant::now());
    let s = &mut app.state;
    s.scroll.heard = heard;
    s.scroll.rate = rate;
    s.scroll.paused = app.transport.scrolling().paused();
    s.scroll.have = (oldest, heard);
    if h <= SHORT + 0.5 {
        s.scroll.wave = None;
        return;
    }
    let wave = Rect::new(ARROW_W + 1.0, super::work_y(s) - h + 1.0, (w - ARROW_W - 2.0).max(1.0), h - 2.0);
    s.scroll.wave = Some(wave);
    let cols = wave.w.round().max(1.0) as usize;
    if s.scroll.cols.len() != cols {
        s.scroll.cols.resize(cols, (0.0, 0.0));
    }
    if rate == 0 {
        s.scroll.cols.fill((0.0, 0.0));
        s.scroll.visible = 0;
        s.scroll.from = 0;
        return;
    }
    let visible = ((shown(s) as f64 * f64::from(rate)) as u64).max(1);
    let from = match s.scroll.hold {
        Some(held) => held,
        None => heard.saturating_sub(visible + s.scroll.pan),
    };
    s.scroll.from = from;
    s.scroll.visible = visible;
    app.transport.scrolling().scan(from, visible, &mut s.scroll.cols);
}

/// Paints the strip over the window's width.
pub fn paint(ui: &mut Ui, s: &State, _f: &FrameInfo) {
    let (w, _) = ui.c.size();
    let r = rect(s, w);
    if r.h <= 0.0 {
        return;
    }
    let (edge, fill, ink) = (ui.t.s(0x1d2022), ui.t.sunken, ui.t.tx(0xb9bfc3));
    ui.c.fill_rect(r, edge);
    ui.c.fill_rect(Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0), fill);

    // The collapse arrow's column, a chevron in its middle.
    let ar = Rect::new(r.x, r.y, ARROW_W, r.h);
    let chev = if s.scroll.collapsed { ">" } else { "<" };
    let cw = ui.c.text_width(fnt(12.0), chev);
    ui.c.text(fnt(12.0), ar.x + (ar.w - cw) / 2.0, r.y + (r.h - 12.0) / 2.0, ink, chev);
    ui.hit(ar, Action::Scrolling(Act::Collapse), if s.scroll.collapsed { "Show the scrolling sampler" } else { "Hide the scrolling sampler" });
    ui.c.fill_rect(Rect::new(ar.x + ar.w, r.y, 1.0, r.h), edge);
    if s.scroll.collapsed {
        return;
    }

    let Some(wv) = s.scroll.wave else { return };
    // The wave: a column a device pixel, min/max of its frames about the middle. A column of nothing outside the
    // tape (before its oldest frame, past the newest heard) draws nothing: it is not silence, it is no tape.
    let mid = wv.y + wv.h / 2.0;
    let amp = wv.h / 2.0 - 1.0;
    let palette = crate::prefs::index(&crate::prefs::SCROLLING_WAVE_MODE);
    let custom = crate::prefs::scrolling_wave_color().map_or(ui.t.accent, os_ui::Color::rgb);
    for (c, &(lo, hi)) in s.scroll.cols.iter().enumerate() {
        let at = s.scroll.from + (s.scroll.visible as f64 * (c as f64 + 0.5) / s.scroll.cols.len().max(1) as f64) as u64;
        if (lo, hi) == (0.0, 0.0) && !(s.scroll.have.0..s.scroll.have.1).contains(&at) {
            continue;
        }
        let (lo, hi) = (lo.clamp(-1.0, 1.0), hi.clamp(-1.0, 1.0));
        let y0 = mid - hi * amp;
        let y1 = mid - lo * amp;
        if palette == 2 {
            let hue = spectrum_hue(c, s.scroll.cols.len());
            let fill = spectrum_color(hue, 0.30);
            let outline = spectrum_color(hue, 0.94);
            let top = y0.min(y1);
            let height = (y1 - y0).abs().max(1.0);
            ui.c.fill_cell(Rect::new(wv.x + c as f32, top, 1.0, height), fill);
            ui.c.fill_cell(Rect::new(wv.x + c as f32, top, 1.0, 1.0), outline);
            if height > 1.0 {
                ui.c.fill_cell(Rect::new(wv.x + c as f32, top + height - 1.0, 1.0, 1.0), outline);
            }
            continue;
        }
        let ink = if palette == 1 {
            custom
        } else {
            ui.t.accent
        };
        let fill = ink.with_alpha(30);
        let outline = ink.with_alpha(240);
        let top = y0.min(y1);
        let height = (y1 - y0).abs().max(1.0);
        ui.c.fill_cell(Rect::new(wv.x + c as f32, top, 1.0, height), fill);
        ui.c.fill_cell(Rect::new(wv.x + c as f32, top, 1.0, 1.0), outline);
        if height > 1.0 {
            ui.c.fill_cell(Rect::new(wv.x + c as f32, top + height - 1.0, 1.0, 1.0), outline);
        }
    }
    // Paused (a Ctrl+click on the wave): the newest audio is not taken while it stands.
    if s.scroll.paused {
        let text = "Paused";
        let tw = ui.c.text_width(fnt(11.0), text);
        let b = Rect::new(wv.right() - tw - 10.0, wv.y + 3.0, tw + 8.0, 15.0);
        ui.c.fill_rrect(b, 3.0, ui.t.accent);
        ui.c.text(fnt(11.0), b.x + 4.0, b.y + 2.0, ui.t.s(0x101214), text);
    }

    // Time marks back from the newest frame, a step that keeps its labels apart.
    if s.scroll.rate > 0 && s.scroll.visible > 0 {
        let per_px = shown(s) / wv.w.max(1.0);
        let step = [1.0f64, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0]
            .into_iter()
            .find(|&t| t / f64::from(per_px) >= 70.0)
            .unwrap_or(600.0);
        let tape_secs = s.scroll.heard as f64 / f64::from(s.scroll.rate.max(1));
        let mut t = step;
        while t < tape_secs {
            let x = x_of(s, s.scroll.heard - (t * f64::from(s.scroll.rate)) as u64);
            if x < wv.x - 1.0 {
                break;
            }
            ui.c.fill_rect(Rect::new(x, wv.y, 1.0, wv.h), edge);
            let label = if t >= 60.0 && t % 60.0 == 0.0 { format!("-{}:00", t as u64 / 60) } else { format!("-0:{:02}", t as u64) };
            let lw = ui.c.text_width(fnt(11.0), &label);
            ui.c.text(fnt(11.0), (x + 2.0).min(wv.x + wv.w - lw - 2.0), wv.y + 2.0, ink, &label);
            t += step;
        }
    }

    // The selection, its edges accented.
    if let Some((a, b)) = s.scroll.sel {
        let (x0, x1) = (x_of(s, a), x_of(s, b));
        let sel = Rect::new(x0, wv.y, (x1 - x0).max(2.0), wv.h);
        ui.c.fill_rect(sel, ui.t.accent.with_alpha(56));
        ui.c.fill_rect(Rect::new(x0, wv.y, 1.0, wv.h), ui.t.accent);
        ui.c.fill_rect(Rect::new(x1 - 1.0, wv.y, 1.0, wv.h), ui.t.accent);
    }

    let hint = if s.scroll.paused {
        "Scrolling sampler paused — Ctrl+click to keep taking the newest output"
    } else if s.scroll.sel.is_some() {
        "Scrolling sampler — drag the selection onto a playlist track, click beside it to select again, Ctrl+click to pause"
    } else {
        "Scrolling sampler — drag to select a piece of the recent output, then drag it onto a playlist track; Ctrl+click pauses"
    };
    ui.hit(wv, Action::Scrolling(Act::Wave), hint);
}

fn spectrum_color(hue: f32, alpha: f32) -> os_ui::Color {
    os_ui::Color::from_hsl_pub(hue.rem_euclid(1.0) * 360.0, 1.0, 0.5, (alpha * 255.0) as u8)
}

/// Fixed ROYGBIV mask across the visible strip: red at the left edge, violet at the right.
fn spectrum_hue(column: usize, columns: usize) -> f32 {
    let span = columns.saturating_sub(1).max(1) as f32;
    (column as f32 / span).clamp(0.0, 1.0) * (5.0 / 6.0)
}

#[cfg(test)]
mod tests {
    use super::{spectrum_color, spectrum_hue};

    #[test]
    fn rainbow_palette_wraps_smoothly_and_preserves_alpha() {
        let a = spectrum_color(0.0, 0.5);
        let b = spectrum_color(1.0, 0.5);
        assert_eq!((a.r, a.g, a.b, a.a), (b.r, b.g, b.b, b.a));
        assert_eq!(a.a, 127);
        let green = spectrum_color(1.0 / 3.0, 0.5);
        let blue = spectrum_color(2.0 / 3.0, 0.5);
        assert!(a.r > a.g && a.r > a.b);
        assert!(green.g > green.r && green.g > green.b);
        assert!(blue.b > blue.r && blue.b > blue.g);
    }

    #[test]
    fn rainbow_mask_maps_visible_width_from_red_to_violet() {
        assert_eq!(spectrum_hue(0, 701), 0.0);
        assert!((spectrum_hue(350, 701) - 5.0 / 12.0).abs() < 0.001);
        assert!((spectrum_hue(700, 701) - 5.0 / 6.0).abs() < 0.001);
        assert_eq!(spectrum_hue(0, 1), 0.0);
    }
}
