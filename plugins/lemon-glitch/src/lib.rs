//! An original 16-step, tempo-synced effect sequencer with four independently enabled processing lanes.
//! The shared small-effect framework supplies its editor and saved state.
#![allow(semicolon_in_expressions_from_non_local_macros)]

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use lemon_small_fx::editor::FxEditor;
use lemon_small_fx::params::FxParams;
use lemon_small_fx::{Card, Control, Mark, Setting, Shared, Sound, Spec};
use nih_plug::prelude::*;
use os_ui::Rect;
mod dsp;
mod units;
use dsp::{GlitchDsp, HostPosition};
use lemon_small_fx::{PanelItem, PanelKind, PanelLane};
use units::Unit;

const LANES: usize = 11;
const MAX_STEPS: usize = 32;
const STEP_SETTINGS: usize = LANES * MAX_STEPS;
const ENABLE_BASE: usize = STEP_SETTINGS;
const AMOUNT_BASE: usize = ENABLE_BASE + LANES;
const AUX_BASE: usize = AMOUNT_BASE + LANES;
const GRID_LENGTH: usize = AUX_BASE + LANES;
const BARS: usize = GRID_LENGTH + 1;
const DIVISION: usize = BARS + 1;
const MIX: usize = DIVISION + 1;
const DELAY_TIME_L: usize = MIX + 1;
const DELAY_TIME_R: usize = MIX + 2;
const DELAY_FILTER: usize = MIX + 3;
const EXTRA_BASE: usize = MIX + 4;
const RAND_CHANGE: usize = EXTRA_BASE;
const DIST_TYPE: usize = EXTRA_BASE + 1;
const DIST_LEVEL: usize = EXTRA_BASE + 2;
const DIST_LOWCUT: usize = EXTRA_BASE + 3;
const SHUF_ORDER: usize = EXTRA_BASE + 4;
const SHUF_FADE: usize = EXTRA_BASE + 5;
const STRETCH_PITCH: usize = EXTRA_BASE + 6;
const STRETCH_SEARCH: usize = EXTRA_BASE + 7;
const REV_LENGTH: usize = EXTRA_BASE + 8;
const REV_PITCH: usize = EXTRA_BASE + 9;
const RETRIG_SLIDE: usize = EXTRA_BASE + 10;
const RETRIG_DECAY: usize = EXTRA_BASE + 11;
const GATE_RELEASE: usize = EXTRA_BASE + 12;
const GATE_DEPTH: usize = EXTRA_BASE + 13;
const GATE_RATE: usize = EXTRA_BASE + 14;
const DELAY_LEVEL: usize = EXTRA_BASE + 15;
const LOFI_NOISE: usize = EXTRA_BASE + 16;
const LOFI_TONE: usize = EXTRA_BASE + 17;
const TAPE_CURVE: usize = EXTRA_BASE + 18;
const TAPE_FINAL: usize = EXTRA_BASE + 19;
const MOD_RATE: usize = EXTRA_BASE + 20;
const MOD_SHAPE: usize = EXTRA_BASE + 21;
const MOD_STEREO: usize = EXTRA_BASE + 22;
const PARAMS: usize = MOD_STEREO + 1;
const RANDOMIZER: usize = 0;
const DISTORTION: usize = 1;
const SHUFFLER: usize = 2;
const STRETCHER: usize = 3;
const REVERSER: usize = 4;
const RETRIGGER: usize = 5;
const GATER: usize = 6;
const DELAY: usize = 7;
const LOFI: usize = 8;
const TAPE_STOP: usize = 9;
const MODULATOR: usize = 10;
const LANE_LABELS: [&str; LANES] = [
    "Randomizer",
    "Distortion",
    "Shuffler",
    "Stretcher",
    "Reverser",
    "Retrigger",
    "Gater",
    "Delay",
    "Lofi",
    "Tape Stop",
    "Modulator",
];
const ENABLE_IDS: [&str; LANES] = [
    "randomizer_on",
    "distortion_on",
    "shuffler_on",
    "stretcher_on",
    "reverser_on",
    "retrigger_on",
    "gater_on",
    "delay_on",
    "lofi_on",
    "tape_stop_on",
    "modulator_on",
];
#[cfg(test)]
const AMOUNT_IDS: [&str; LANES] = [
    "randomizer_amount",
    "distortion_amount",
    "shuffler_amount",
    "stretcher_amount",
    "reverser_amount",
    "retrigger_amount",
    "gater_amount",
    "delay_amount",
    "lofi_amount",
    "tape_stop_amount",
    "modulator_amount",
];
#[cfg(test)]
const AUX_IDS: [&str; LANES] = [
    "randomizer_aux",
    "distortion_aux",
    "shuffler_aux",
    "stretcher_aux",
    "reverser_aux",
    "retrigger_aux",
    "gater_aux",
    "delay_aux",
    "lofi_aux",
    "tape_stop_aux",
    "modulator_aux",
];
const DIVISIONS: [&str; 4] = ["1/8", "1/16", "1/32", "1/64"];
const BAR_COUNTS: [&str; 3] = ["1 bar", "2 bars", "4 bars"];
const GRID_LENGTHS: [&str; 2] = ["16 steps", "32 steps"];
const CHANGE_MODES: [&str; 2] = ["Per step", "Per division"];
const DIST_TYPES: [&str; 3] = ["Soft", "Hard", "Fold"];
const SLICE_COUNTS: [&str; 7] = ["2", "3", "4", "6", "8", "12", "16"];
const SLICE_VALUES: [usize; 7] = [2, 3, 4, 6, 8, 12, 16];
const SHUFFLE_ORDERS: [&str; 4] = ["Pairs", "Reverse", "Rotate", "Random"];
const REVERSE_LENGTHS: [&str; 4] = ["1/8 step", "1/4 step", "1/2 step", "1 step"];
const DELAY_MODES: [&str; 2] = ["Stereo", "Ping-pong"];
const GATE_RATES: [&str; 3] = ["x1", "x2", "x4"];
const MODULATOR_MODES: [&str; 3] = ["AM", "Ring", "Vibrato"];
const LFO_SHAPES: [&str; 4] = ["Sine", "Triangle", "Square", "Random"];

const fn lin(lo: f64, hi: f64, prec: usize, suffix: &'static str) -> Unit {
    Unit::Lin {
        lo,
        hi,
        prec,
        suffix,
    }
}
const fn exp(lo: f64, hi: f64, prec: usize, suffix: &'static str) -> Unit {
    Unit::Exp {
        lo,
        hi,
        prec,
        suffix,
    }
}

const AMOUNT_UNITS: [Unit; LANES] = [
    Unit::Percent,
    lin(0.0, 30.0, 1, " dB"),
    Unit::Names(&SLICE_COUNTS),
    exp(1.0, 8.0, 2, "x"),
    Unit::Percent,
    Unit::Count { lo: 1, suffix: "x" },
    lin(5.0, 100.0, 0, "%"),
    Unit::Percent,
    lin(16.0, 3.0, 1, " bit"),
    lin(10.0, 100.0, 0, "% of step"),
    Unit::Percent,
];
const AUX_UNITS: [Unit; LANES] = [
    Unit::Plain,
    exp(500.0, 20000.0, 0, " Hz"),
    Unit::Percent,
    exp(20.0, 300.0, 0, " ms"),
    lin(0.5, 30.0, 1, " ms"),
    lin(-12.0, 12.0, 2, " st"),
    exp(0.1, 50.0, 1, " ms"),
    Unit::Names(&DELAY_MODES),
    exp(1.0, 64.0, 1, ":1"),
    exp(10.0, 800.0, 0, " ms"),
    Unit::Names(&MODULATOR_MODES),
];
const EXTRA_UNITS: [Unit; PARAMS - EXTRA_BASE] = [
    Unit::Names(&CHANGE_MODES),
    Unit::Names(&DIST_TYPES),
    lin(-24.0, 6.0, 1, " dB"),
    exp(20.0, 1000.0, 0, " Hz"),
    Unit::Names(&SHUFFLE_ORDERS),
    lin(0.5, 20.0, 1, " ms"),
    lin(-12.0, 12.0, 2, " st"),
    Unit::Percent,
    Unit::Names(&REVERSE_LENGTHS),
    lin(-12.0, 12.0, 2, " st"),
    lin(-3.0, 3.0, 2, " st"),
    Unit::Percent,
    exp(0.1, 100.0, 1, " ms"),
    Unit::Percent,
    Unit::Names(&GATE_RATES),
    Unit::Percent,
    Unit::Percent,
    exp(1000.0, 20000.0, 0, " Hz"),
    exp(0.5, 4.0, 2, ""),
    Unit::Percent,
    exp(0.1, 2000.0, 1, " Hz"),
    Unit::Names(&LFO_SHAPES),
    lin(0.0, 180.0, 0, "°"),
];

fn unit(k: usize) -> Unit {
    match k {
        AMOUNT_BASE..AUX_BASE => AMOUNT_UNITS[k - AMOUNT_BASE],
        AUX_BASE..GRID_LENGTH => AUX_UNITS[k - AUX_BASE],
        GRID_LENGTH => Unit::Names(&GRID_LENGTHS),
        BARS => Unit::Names(&BAR_COUNTS),
        DIVISION => Unit::Names(&DIVISIONS),
        DELAY_TIME_L | DELAY_TIME_R => exp(0.25, 4.0, 2, "x"),
        DELAY_FILTER => exp(250.0, 18000.0, 0, " Hz"),
        EXTRA_BASE..PARAMS => EXTRA_UNITS[k - EXTRA_BASE],
        _ => Unit::Percent,
    }
}

fn top(k: usize) -> i32 {
    SETTINGS[k].range.1
}

/// A setting's value in its unit (milliseconds, semitones, a name's index...).
fn value(values: &[i32], k: usize) -> f64 {
    unit(k).value(values[k], top(k))
}

macro_rules! step_setting {
    ($lane:literal, $label:literal, $step:literal) => {
        Setting {
            id: concat!($lane, "_step_", $step),
            name: concat!($label, " step ", $step),
            range: (0, 10000),
            start: 0,
            magnet: None,
        }
    };
}
macro_rules! step_ids {
    ($lane:literal) => {
        &[
            concat!($lane, "_step_01"),
            concat!($lane, "_step_02"),
            concat!($lane, "_step_03"),
            concat!($lane, "_step_04"),
            concat!($lane, "_step_05"),
            concat!($lane, "_step_06"),
            concat!($lane, "_step_07"),
            concat!($lane, "_step_08"),
            concat!($lane, "_step_09"),
            concat!($lane, "_step_10"),
            concat!($lane, "_step_11"),
            concat!($lane, "_step_12"),
            concat!($lane, "_step_13"),
            concat!($lane, "_step_14"),
            concat!($lane, "_step_15"),
            concat!($lane, "_step_16"),
            concat!($lane, "_step_17"),
            concat!($lane, "_step_18"),
            concat!($lane, "_step_19"),
            concat!($lane, "_step_20"),
            concat!($lane, "_step_21"),
            concat!($lane, "_step_22"),
            concat!($lane, "_step_23"),
            concat!($lane, "_step_24"),
            concat!($lane, "_step_25"),
            concat!($lane, "_step_26"),
            concat!($lane, "_step_27"),
            concat!($lane, "_step_28"),
            concat!($lane, "_step_29"),
            concat!($lane, "_step_30"),
            concat!($lane, "_step_31"),
            concat!($lane, "_step_32"),
        ]
    };
}

const STEP_IDS: [&[&str]; LANES] = [
    step_ids!("randomizer"),
    step_ids!("distortion"),
    step_ids!("shuffler"),
    step_ids!("stretcher"),
    step_ids!("reverser"),
    step_ids!("retrigger"),
    step_ids!("gater"),
    step_ids!("delay"),
    step_ids!("lofi"),
    step_ids!("tape_stop"),
    step_ids!("modulator"),
];

macro_rules! all_settings {
    ($(($prefix:literal, $label:literal, $amount:literal, $aux:literal, $amount_max:literal, $amount_start:literal, $aux_max:literal, $aux_start:literal, $aux_magnet:expr)),+ $(,)?) => {
        [
            $(
                step_setting!($prefix, $label, "01"), step_setting!($prefix, $label, "02"), step_setting!($prefix, $label, "03"), step_setting!($prefix, $label, "04"),
                step_setting!($prefix, $label, "05"), step_setting!($prefix, $label, "06"), step_setting!($prefix, $label, "07"), step_setting!($prefix, $label, "08"),
                step_setting!($prefix, $label, "09"), step_setting!($prefix, $label, "10"), step_setting!($prefix, $label, "11"), step_setting!($prefix, $label, "12"),
                step_setting!($prefix, $label, "13"), step_setting!($prefix, $label, "14"), step_setting!($prefix, $label, "15"), step_setting!($prefix, $label, "16"),
                step_setting!($prefix, $label, "17"), step_setting!($prefix, $label, "18"), step_setting!($prefix, $label, "19"), step_setting!($prefix, $label, "20"),
                step_setting!($prefix, $label, "21"), step_setting!($prefix, $label, "22"), step_setting!($prefix, $label, "23"), step_setting!($prefix, $label, "24"),
                step_setting!($prefix, $label, "25"), step_setting!($prefix, $label, "26"), step_setting!($prefix, $label, "27"), step_setting!($prefix, $label, "28"),
                step_setting!($prefix, $label, "29"), step_setting!($prefix, $label, "30"), step_setting!($prefix, $label, "31"), step_setting!($prefix, $label, "32"),
            )+
            $(
                Setting { id: concat!($prefix, "_on"), name: concat!($label, " enabled"), range: (0, 10000), start: 0, magnet: None },
            )+
            $(
                Setting { id: concat!($prefix, "_amount"), name: $amount, range: (0, $amount_max), start: $amount_start, magnet: None },
            )+
            $(
                Setting { id: concat!($prefix, "_aux"), name: $aux, range: (0, $aux_max), start: $aux_start, magnet: $aux_magnet },
            )+
            Setting { id: "grid_length", name: "Step count", range: (0, 1), start: 0, magnet: None },
            Setting { id: "bars", name: "Pattern bars", range: (0, 2), start: 0, magnet: None },
            Setting { id: "division", name: "Sub-division", range: (0, 3), start: 1, magnet: None },
            Setting { id: "mix", name: "Mix", range: (0, 10000), start: 10000, magnet: None },
            Setting { id: "delay_time_l", name: "Delay left", range: (0, 10000), start: 5000, magnet: Some(5000) },
            Setting { id: "delay_time_r", name: "Delay right", range: (0, 10000), start: 5000, magnet: Some(5000) },
            Setting { id: "delay_filter", name: "Delay feedback filter", range: (0, 10000), start: 8000, magnet: None },
            Setting { id: "randomizer_change", name: "Randomizer change", range: (0, 1), start: 0, magnet: None },
            Setting { id: "distortion_type", name: "Distortion type", range: (0, 2), start: 0, magnet: None },
            Setting { id: "distortion_level", name: "Distortion level", range: (0, 10000), start: 8000, magnet: Some(8000) },
            Setting { id: "distortion_lowcut", name: "Distortion low cut", range: (0, 10000), start: 0, magnet: None },
            Setting { id: "shuffler_order", name: "Shuffler order", range: (0, 3), start: 0, magnet: None },
            Setting { id: "shuffler_fade", name: "Shuffler fade", range: (0, 10000), start: 3000, magnet: None },
            Setting { id: "stretcher_pitch", name: "Stretcher pitch", range: (0, 10000), start: 5000, magnet: Some(5000) },
            Setting { id: "stretcher_search", name: "Stretcher search", range: (0, 10000), start: 5000, magnet: None },
            Setting { id: "reverser_length", name: "Reverser length", range: (0, 3), start: 3, magnet: None },
            Setting { id: "reverser_pitch", name: "Reverser pitch", range: (0, 10000), start: 5000, magnet: Some(5000) },
            Setting { id: "retrigger_slide", name: "Retrigger pitch slide", range: (0, 10000), start: 5000, magnet: Some(5000) },
            Setting { id: "retrigger_decay", name: "Retrigger decay", range: (0, 10000), start: 0, magnet: None },
            Setting { id: "gater_release", name: "Gater release", range: (0, 10000), start: 3000, magnet: None },
            Setting { id: "gater_depth", name: "Gater depth", range: (0, 10000), start: 10000, magnet: None },
            Setting { id: "gater_rate", name: "Gater rate", range: (0, 2), start: 0, magnet: None },
            Setting { id: "delay_level", name: "Delay level", range: (0, 10000), start: 7000, magnet: None },
            Setting { id: "lofi_noise", name: "Lofi noise", range: (0, 10000), start: 0, magnet: None },
            Setting { id: "lofi_tone", name: "Lofi tone", range: (0, 10000), start: 10000, magnet: None },
            Setting { id: "tape_stop_curve", name: "Tape stop curve", range: (0, 10000), start: 3333, magnet: Some(3333) },
            Setting { id: "tape_stop_final", name: "Tape stop final speed", range: (0, 10000), start: 0, magnet: None },
            Setting { id: "modulator_rate", name: "Modulator rate", range: (0, 10000), start: 4000, magnet: None },
            Setting { id: "modulator_shape", name: "Modulator shape", range: (0, 3), start: 0, magnet: None },
            Setting { id: "modulator_stereo", name: "Modulator stereo phase", range: (0, 10000), start: 0, magnet: None },
        ]
    };
}

const SETTINGS: [Setting; PARAMS] = all_settings!(
    (
        "randomizer",
        "Randomizer",
        "Probability",
        "Seed",
        10000,
        10000,
        10000,
        4700,
        None
    ),
    (
        "distortion",
        "Distortion",
        "Drive",
        "Tone",
        10000,
        4000,
        10000,
        7000,
        None
    ),
    (
        "shuffler", "Shuffler", "Slices", "Shuffle", 6, 4, 10000, 10000, None
    ),
    (
        "stretcher",
        "Stretcher",
        "Stretch",
        "Grain size",
        10000,
        3333,
        10000,
        5000,
        None
    ),
    (
        "reverser",
        "Reverser",
        "Reverse mix",
        "Crossfade",
        10000,
        10000,
        10000,
        3000,
        None
    ),
    (
        "retrigger",
        "Retrigger",
        "Repeats",
        "Pitch",
        15,
        3,
        10000,
        5000,
        Some(5000)
    ),
    (
        "gater", "Gater", "Duty", "Attack", 10000, 5000, 10000, 1000, None
    ),
    (
        "delay",
        "Delay",
        "Feedback",
        "Ping-pong",
        10000,
        4000,
        1,
        0,
        None
    ),
    (
        "lofi",
        "Lofi",
        "Bit depth",
        "Rate reduction",
        10000,
        5000,
        10000,
        4000,
        None
    ),
    (
        "tape_stop",
        "Tape Stop",
        "Stop time",
        "Recovery",
        10000,
        5000,
        10000,
        5000,
        None
    ),
    (
        "modulator",
        "Modulator",
        "Depth",
        "Mode",
        10000,
        5000,
        2,
        0,
        None
    ),
);

const PANEL_TOP: f32 = 300.0;
const TRANSPORT_TOP: f32 = 420.0;

const CARDS: [Card; 3] = [
    Card {
        x0: 8.0,
        x1: 752.0,
        y0: 30.0,
        y1: 274.0,
        heading: "SEQUENCER",
    },
    Card {
        x0: 8.0,
        x1: 752.0,
        y0: PANEL_TOP,
        y1: PANEL_TOP + 100.0,
        heading: "",
    },
    Card {
        x0: 8.0,
        x1: 752.0,
        y0: TRANSPORT_TOP,
        y1: TRANSPORT_TOP + 96.0,
        heading: "TRANSPORT",
    },
];

const fn knob(id: &'static str, caption: &'static str) -> PanelItem {
    PanelItem {
        id,
        caption,
        kind: PanelKind::Knob { bipolar: false },
    }
}
const fn pitch_knob(id: &'static str, caption: &'static str) -> PanelItem {
    PanelItem {
        id,
        caption,
        kind: PanelKind::Knob { bipolar: true },
    }
}
const fn choice(
    id: &'static str,
    caption: &'static str,
    names: &'static [&'static str],
) -> PanelItem {
    PanelItem {
        id,
        caption,
        kind: PanelKind::Choice { names },
    }
}

/// What each lane's button opens, in the order of the lanes: the settings that act on that effect alone.
const PANEL_LANES: [PanelLane; LANES] = [
    PanelLane {
        label: "Randomizer",
        items: &[
            choice("randomizer_change", "CHANGE", &CHANGE_MODES),
            knob("randomizer_amount", "PROBABILITY"),
            knob("randomizer_aux", "SEED"),
        ],
    },
    PanelLane {
        label: "Distortion",
        items: &[
            choice("distortion_type", "TYPE", &DIST_TYPES),
            knob("distortion_amount", "DRIVE"),
            knob("distortion_lowcut", "LOW CUT"),
            knob("distortion_aux", "TONE"),
            knob("distortion_level", "LEVEL"),
        ],
    },
    PanelLane {
        label: "Shuffler",
        items: &[
            choice("shuffler_order", "ORDER", &SHUFFLE_ORDERS),
            knob("shuffler_amount", "SLICES"),
            knob("shuffler_aux", "SHUFFLE"),
            knob("shuffler_fade", "FADE"),
        ],
    },
    PanelLane {
        label: "Stretcher",
        items: &[
            knob("stretcher_amount", "STRETCH"),
            knob("stretcher_aux", "GRAIN"),
            pitch_knob("stretcher_pitch", "PITCH"),
            knob("stretcher_search", "SEARCH"),
        ],
    },
    PanelLane {
        label: "Reverser",
        items: &[
            choice("reverser_length", "LENGTH", &REVERSE_LENGTHS),
            knob("reverser_amount", "MIX"),
            knob("reverser_aux", "CROSSFADE"),
            pitch_knob("reverser_pitch", "PITCH"),
        ],
    },
    PanelLane {
        label: "Retrigger",
        items: &[
            knob("retrigger_amount", "REPEATS"),
            pitch_knob("retrigger_aux", "PITCH"),
            pitch_knob("retrigger_slide", "SLIDE"),
            knob("retrigger_decay", "DECAY"),
        ],
    },
    PanelLane {
        label: "Gater",
        items: &[
            choice("gater_rate", "RATE", &GATE_RATES),
            knob("gater_amount", "DUTY"),
            knob("gater_aux", "ATTACK"),
            knob("gater_release", "RELEASE"),
            knob("gater_depth", "DEPTH"),
        ],
    },
    PanelLane {
        label: "Delay",
        items: &[
            choice("delay_aux", "MODE", &DELAY_MODES),
            knob("delay_amount", "FEEDBACK"),
            knob("delay_time_l", "TIME L"),
            knob("delay_time_r", "TIME R"),
            knob("delay_filter", "TONE"),
            knob("delay_level", "LEVEL"),
        ],
    },
    PanelLane {
        label: "Lofi",
        items: &[
            knob("lofi_amount", "BITS"),
            knob("lofi_aux", "RATE"),
            knob("lofi_noise", "NOISE"),
            knob("lofi_tone", "TONE"),
        ],
    },
    PanelLane {
        label: "Tape Stop",
        items: &[
            knob("tape_stop_amount", "STOP TIME"),
            knob("tape_stop_curve", "CURVE"),
            knob("tape_stop_final", "STOP TO"),
            knob("tape_stop_aux", "RECOVERY"),
        ],
    },
    PanelLane {
        label: "Modulator",
        items: &[
            choice("modulator_aux", "MODE", &MODULATOR_MODES),
            choice("modulator_shape", "SHAPE", &LFO_SHAPES),
            knob("modulator_amount", "DEPTH"),
            knob("modulator_rate", "RATE"),
            knob("modulator_stereo", "STEREO"),
        ],
    },
];

const CONTROLS: [Control; 6] = [
    Control::StepGrid {
        ids: &STEP_IDS,
        labels: &LANE_LABELS,
        enabled_ids: &ENABLE_IDS,
        columns: MAX_STEPS,
        rows: LANES,
        length_id: "grid_length",
        rect: Rect::new(192.0, 42.0, 480.0, 220.0),
    },
    Control::Panel {
        lanes: &PANEL_LANES,
        rect: Rect::new(8.0, PANEL_TOP, 744.0, 100.0),
    },
    Control::Choice {
        id: "grid_length",
        x: 24.0,
        top: TRANSPORT_TOP + 20.0,
        names: &GRID_LENGTHS,
    },
    Control::Choice {
        id: "bars",
        x: 176.0,
        top: TRANSPORT_TOP + 20.0,
        names: &BAR_COUNTS,
    },
    Control::Choice {
        id: "division",
        x: 360.0,
        top: TRANSPORT_TOP + 20.0,
        names: &DIVISIONS,
    },
    Control::Knob {
        id: "mix",
        cx: 520.0,
        cy: TRANSPORT_TOP + 44.0,
        r: 14.0,
        caption: "MIX",
        bipolar: false,
    },
];

fn text(k: usize, value: i32) -> String {
    match k {
        0..AMOUNT_BASE => if value > 0 { "On" } else { "Off" }.into(),
        _ => unit(k).show(value, top(k)),
    }
}

fn parse(k: usize, text: &str) -> Option<i32> {
    match k {
        0..AMOUNT_BASE => match text.trim().to_ascii_lowercase().as_str() {
            "on" | "1" => Some(10000),
            "off" | "0" => Some(0),
            _ => None,
        },
        _ => unit(k).parse(text, top(k)),
    }
}

fn sound(rate: u32, values: &[i32]) -> Box<dyn Sound> {
    Box::new(GlitchDsp::new(rate, values))
}

pub const SPEC: &Spec = &Spec {
    name: "Lime Glitch",
    word: "GLITCH",
    about: "Lime Studio's tempo-synced step sequencer",
    settings: &SETTINGS,
    version: None,
    any_version: false,
    fields: &[],
    tail: &[],
    blank: None,
    locate: None,
    size: (760, 538),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark {
        x: 736.0,
        y: TRANSPORT_TOP + 48.0,
        anchor: 1.0,
        stacked: false,
    },
    hint: None,
    labels: &[],
    text,
    parse,
    shown: None,
    sound,
    rings: false,
};

pub struct LemonGlitch {
    params: Arc<FxParams>,
    shared: Arc<Shared>,
    dsp: Option<GlitchDsp>,
    raw: Vec<u8>,
    raw_seen: u64,
}

impl Default for LemonGlitch {
    fn default() -> Self {
        let params = Arc::new(FxParams::new(SPEC));
        let raw = params.raw_copy();
        LemonGlitch {
            params,
            shared: Arc::new(Shared::default()),
            dsp: None,
            raw,
            raw_seen: u64::MAX,
        }
    }
}

impl LemonGlitch {
    fn settings(&mut self) -> [i32; PARAMS] {
        let version = self.params.raw_version.load(Ordering::Acquire);
        if version != self.raw_seen {
            if let Ok(raw) = self.params.raw.try_read() {
                self.raw.clone_from(&raw);
                self.raw_seen = version;
            }
        }
        let values = self.params.settings(&self.raw);
        std::array::from_fn(|index| values[index])
    }
}

impl Plugin for LemonGlitch {
    const NAME: &'static str = "Lime Glitch";
    const VENDOR: &'static str = "Lime Studio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(FxEditor::new(
            SPEC,
            self.params.clone(),
            self.shared.clone(),
        )))
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        config: &BufferConfig,
        _ctx: &mut impl InitContext<Self>,
    ) -> bool {
        let rate = config.sample_rate.round() as u32;
        self.shared.rate.store(rate, Ordering::Relaxed);
        self.raw_seen = u64::MAX;
        let values = self.settings();
        match &mut self.dsp {
            Some(dsp) => {
                dsp.set_sample_rate(rate);
                dsp.set(&values);
            }
            None => self.dsp = Some(GlitchDsp::new(rate, &values)),
        }
        true
    }

    fn reset(&mut self) {
        if let Some(dsp) = &mut self.dsp {
            dsp.reset();
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        ctx: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let values = self.settings();
        let Some(dsp) = &mut self.dsp else {
            return ProcessStatus::Normal;
        };
        dsp.set(&values);
        let transport = ctx.transport();
        let host = transport
            .playing
            .then(|| transport.pos_beats())
            .flatten()
            .map(|beats| HostPosition {
                beats,
                bpm: transport.tempo.unwrap_or(120.0),
            });
        if let Some(position) = host {
            let bars = [1.0, 2.0, 4.0][values[BARS].clamp(0, 2) as usize];
            let steps = if values[GRID_LENGTH] == 0 { 16.0 } else { 32.0 };
            let phase = position.beats.rem_euclid(bars * 4.0) / (bars * 4.0);
            self.shared
                .playhead
                .store((phase * steps).floor() as u32, Ordering::Relaxed);
        } else {
            self.shared.playhead.store(u32::MAX, Ordering::Relaxed);
        }
        let channels = buffer.as_slice();
        if channels.len() < 2 {
            return ProcessStatus::Normal;
        }
        let (left, right) = channels.split_at_mut(1);
        dsp.process_at(left[0], right[0], host);
        ProcessStatus::Normal
    }
}

impl ClapPlugin for LemonGlitch {
    const CLAP_ID: &'static str = "org.openstudio.lime-glitch";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Lime Studio's tempo-synced 16-step glitch effect");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Glitch,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for LemonGlitch {
    const VST3_CLASS_ID: [u8; 16] = *b"LimeStudioGlitch";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Modulation];
}

#[cfg(not(feature = "linked"))]
nih_export_clap!(LemonGlitch);
#[cfg(not(feature = "linked"))]
nih_export_vst3!(LemonGlitch);
os_plugkit::linked_clap_entry!(LemonGlitch);

#[cfg(test)]
mod tests {
    use super::*;
    use lemon_small_fx::editor::{Btn, Logic, Mods};
    use lemon_small_fx::view::{Hit, Target};

    #[test]
    fn spec_registers_every_step_and_lane_control_once() {
        assert_eq!(SETTINGS.len(), PARAMS);
        assert_eq!(STEP_SETTINGS, LANES * MAX_STEPS);
        for lane in 0..LANES {
            for step in 0..MAX_STEPS {
                let id = STEP_IDS[lane][step];
                assert_eq!(SPEC.index(id), Some(lane * MAX_STEPS + step));
                assert_eq!(
                    SPEC.controls
                        .iter()
                        .filter(|control| control.contains_id(id))
                        .count(),
                    1
                );
            }
            for id in [ENABLE_IDS[lane], AMOUNT_IDS[lane], AUX_IDS[lane]] {
                assert!(SPEC.index(id).is_some(), "missing {id}");
                assert_eq!(
                    SPEC.controls
                        .iter()
                        .filter(|control| control.contains_id(id))
                        .count(),
                    1
                );
            }
        }
    }

    #[test]
    fn factory_default_uses_known_numeric_settings() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/os-app/factory-presets/Lime Glitch/Default.ospreset");
        let text = std::fs::read_to_string(path).expect("factory preset");
        let mut count = 0;
        for line in text
            .lines()
            .skip(2)
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let (name, value) = line.rsplit_once('=').expect("parameter line");
            let setting = SETTINGS
                .iter()
                .find(|setting| setting.name == name.trim())
                .unwrap_or_else(|| panic!("unknown factory parameter: {}", name.trim()));
            let value = value.trim().parse::<f64>().expect("numeric parameter");
            assert!(
                (setting.range.0 as f64..=setting.range.1 as f64).contains(&value),
                "{} = {value}",
                setting.name
            );
            count += 1;
        }
        assert!(
            count >= 10,
            "factory preset has its pattern and lane settings"
        );
    }

    #[test]
    fn dragging_across_step_cells_paints_a_contiguous_segment() {
        let params = Arc::new(FxParams::new(SPEC));
        let mut logic = Logic::new(SPEC, params, Arc::new(Shared::default()), None);
        logic.hits = vec![
            Hit {
                r: Rect::new(0.0, 0.0, 10.0, 10.0),
                t: Target::Choice("randomizer_step_01".into(), 10000),
            },
            Hit {
                r: Rect::new(10.0, 0.0, 10.0, 10.0),
                t: Target::Choice("randomizer_step_02".into(), 10000),
            },
        ];
        logic.down(5.0, 5.0, Btn::Left, Mods::default(), false);
        logic.moved(15.0, 5.0, Mods::default());
        logic.up(Btn::Left);
        assert_eq!(logic.vals.plain("randomizer_step_01"), 10000.0);
        assert_eq!(logic.vals.plain("randomizer_step_02"), 10000.0);
    }

    #[test]
    fn dragging_a_segment_edge_resizes_the_trigger_run() {
        let params = Arc::new(FxParams::new(SPEC));
        let mut logic = Logic::new(SPEC, params, Arc::new(Shared::default()), None);
        for id in ["randomizer_step_01", "randomizer_step_02"] {
            logic.vals.edit_plain(id, 10000.0);
        }
        logic.hits = vec![
            Hit {
                r: Rect::new(0.0, 0.0, 10.0, 10.0),
                t: Target::Choice("randomizer_step_01".into(), 0),
            },
            Hit {
                r: Rect::new(10.0, 0.0, 10.0, 10.0),
                t: Target::Choice("randomizer_step_02".into(), 0),
            },
            Hit {
                r: Rect::new(20.0, 0.0, 10.0, 10.0),
                t: Target::Choice("randomizer_step_03".into(), 10000),
            },
            Hit {
                r: Rect::new(30.0, 0.0, 10.0, 10.0),
                t: Target::Choice("randomizer_step_04".into(), 10000),
            },
        ];
        logic.down(19.0, 5.0, Btn::Left, Mods::default(), false);
        logic.moved(35.0, 5.0, Mods::default());
        logic.up(Btn::Left);
        for id in [
            "randomizer_step_01",
            "randomizer_step_02",
            "randomizer_step_03",
            "randomizer_step_04",
        ] {
            assert_eq!(logic.vals.plain(id), 10000.0, "{id}");
        }
        assert_eq!(logic.vals.plain("randomizer_step_05"), 0.0);
    }

    fn values(mix: i32) -> [i32; PARAMS] {
        let mut v = std::array::from_fn(|i| SETTINGS[i].start);
        v[MIX] = mix;
        v
    }

    #[test]
    fn disabled_steps_are_bit_exact_passthrough() {
        let mut v = values(10000);
        v[..STEP_SETTINGS].fill(0);
        let mut dsp = GlitchDsp::new(48000, &v);
        let mut left = vec![0.125, -0.75, 0.9, 0.01];
        let mut right = vec![-0.4, 0.2, 0.0, 0.8];
        let before = (left.clone(), right.clone());
        dsp.process_at(
            &mut left,
            &mut right,
            Some(HostPosition {
                beats: 0.0,
                bpm: 120.0,
            }),
        );
        assert_eq!((left, right), before);
    }

    #[test]
    fn mix_zero_is_bit_exact_passthrough() {
        let mut dsp = GlitchDsp::new(48000, &values(0));
        let mut left = vec![0.125, -0.75, 0.9, 0.01];
        let mut right = vec![-0.4, 0.2, 0.0, 0.8];
        let before = (left.clone(), right.clone());
        dsp.process_at(
            &mut left,
            &mut right,
            Some(HostPosition {
                beats: 0.0,
                bpm: 120.0,
            }),
        );
        assert_eq!((left, right), before);
    }

    #[test]
    fn retrigger_and_crush_lanes_can_layer_on_one_step() {
        let mut v = values(10000);
        v[ENABLE_BASE + RETRIGGER] = 10000;
        v[ENABLE_BASE + LOFI] = 10000;
        v[RETRIGGER * MAX_STEPS] = 10000;
        v[LOFI * MAX_STEPS] = 10000;
        v[AMOUNT_BASE + RETRIGGER] = 10000;
        v[AMOUNT_BASE + LOFI] = 10000;
        let mut dsp = GlitchDsp::new(64, &v);
        let mut prime_l = vec![0.9; 32];
        let mut prime_r = vec![-0.7; 32];
        dsp.process_at(
            &mut prime_l,
            &mut prime_r,
            Some(HostPosition {
                beats: 0.0,
                bpm: 120.0,
            }),
        );
        let mut left = vec![0.3; 32];
        let mut right = vec![0.2; 32];
        dsp.process_at(
            &mut left,
            &mut right,
            Some(HostPosition {
                beats: 4.0,
                bpm: 120.0,
            }),
        );
        assert!(
            left[..8].iter().any(|sample| (*sample - 0.3).abs() > 0.01),
            "{left:?}"
        );
        assert!(
            right[..8].iter().any(|sample| (*sample - 0.2).abs() > 0.01),
            "{right:?}"
        );
    }

    #[test]
    fn step_pattern_tracks_host_bar_position() {
        let mut v = values(10000);
        v[..STEP_SETTINGS].fill(0);
        v[GATER * MAX_STEPS + 4] = 10000;
        v[ENABLE_BASE + GATER] = 10000;
        v[AMOUNT_BASE + GATER] = 0;
        v[AUX_BASE + GATER] = 0;
        v[GATE_RELEASE] = 0;
        let mut dsp = GlitchDsp::new(48000, &v);
        let mut left = vec![1.0; 1024];
        let mut right = vec![1.0; 1024];
        dsp.process_at(
            &mut left,
            &mut right,
            Some(HostPosition {
                beats: 1.078125,
                bpm: 120.0,
            }),
        );
        assert!(
            left[512..].iter().all(|sample| *sample < 0.01),
            "step 5 gates the off portion: first {}, later {}",
            left[0],
            left[512]
        );
    }

    fn only_lane(lane: usize) -> [i32; PARAMS] {
        let mut v = values(10000);
        v[..STEP_SETTINGS].fill(0);
        v[lane * MAX_STEPS..lane * MAX_STEPS + 16].fill(10000);
        v[ENABLE_BASE..ENABLE_BASE + LANES].fill(0);
        v[ENABLE_BASE + lane] = 10000;
        v
    }

    #[test]
    fn stretcher_is_transparent_at_one_x_and_zero_semitones() {
        let mut v = only_lane(STRETCHER);
        v[AMOUNT_BASE + STRETCHER] = 0;
        v[STRETCH_PITCH] = 5000;
        let mut dsp = GlitchDsp::new(48000, &v);
        let input: Vec<f32> = (0..48000)
            .map(|n| (n as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 0.5)
            .collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        dsp.process_at(&mut l, &mut r, None);
        let err: f32 = input[12000..]
            .iter()
            .zip(&l[12000..])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(err < 0.05, "max deviation {err}");
    }

    #[test]
    fn stretcher_slows_the_source_down() {
        let mut v = only_lane(STRETCHER);
        v[AMOUNT_BASE + STRETCHER] = 10000;
        v[STRETCH_PITCH] = 5000;
        let mut dsp = GlitchDsp::new(48000, &v);
        let rate = 48000.0;
        let input: Vec<f32> = (0..48000)
            .map(|n| (n as f32 * 440.0 * std::f32::consts::TAU / rate).sin() * 0.5)
            .collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        dsp.process_at(&mut l, &mut r, None);
        let crossings = |s: &[f32]| s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        // Stretching leaves the pitch alone, so the tone keeps its frequency and stays audible.
        let (a, b) = (crossings(&input[20000..40000]), crossings(&l[20000..40000]));
        assert!(a.abs_diff(b) <= 3, "{a} vs {b}");
        assert!(l[20000..40000].iter().any(|x| x.abs() > 0.3));
    }

    #[test]
    fn reverser_plays_the_past_backwards() {
        let mut v = only_lane(REVERSER);
        v[AMOUNT_BASE + REVERSER] = 10000;
        v[REV_PITCH] = 5000;
        v[REV_LENGTH] = 3;
        let mut dsp = GlitchDsp::new(48000, &v);
        let ramp: Vec<f32> = (0..24000).map(|n| n as f32 / 24000.0).collect();
        let (mut l, mut r) = (ramp.clone(), ramp.clone());
        dsp.process_at(&mut l, &mut r, None);
        let mid = &l[19000..19400];
        assert!(
            mid.windows(2).all(|w| w[1] < w[0]),
            "not falling {:?}",
            &mid[..6]
        );
    }

    #[test]
    fn randomizer_plays_effects_whose_own_steps_are_empty() {
        let mut v = only_lane(RANDOMIZER);
        v[AMOUNT_BASE + RANDOMIZER] = 10000;
        let mut dsp = GlitchDsp::new(48000, &v);
        let input: Vec<f32> = (0..96000)
            .map(|n| (n as f32 * 220.0 * std::f32::consts::TAU / 48000.0).sin() * 0.5)
            .collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        dsp.process_at(&mut l, &mut r, None);
        assert!(l.iter().zip(&input).any(|(a, b)| (a - b).abs() > 0.01));
    }

    #[test]
    #[ignore]
    fn render_editor() {
        let Some(dir) = std::env::var_os("GLITCH_PNG") else {
            return;
        };
        for lane in [0usize, 3, 7] {
            let mut l = lemon_small_fx::editor::Logic::new(
                &SPEC,
                Arc::new(FxParams::new(&SPEC)),
                Arc::new(Shared::default()),
                None,
            );
            l.view.selected = lane;
            let (w, h) = (SPEC.size.0 * 2, SPEC.size.1 * 2);
            let mut c = os_ui::Canvas::new(w, h, 2.0).unwrap();
            l.paint(&mut c, &os_plugkit::palette());
            let mut px = vec![0u32; (w * h) as usize];
            c.copy_to_xrgb(&mut px);
            let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
            for (o, p) in pm.data_mut().chunks_mut(4).zip(&px) {
                o.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
            }
            pm.save_png(std::path::Path::new(&dir).join(format!("lane{lane}.png")))
                .unwrap();
        }
    }
}
