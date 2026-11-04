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
use dsp::{GlitchDsp, HostPosition};

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
const PARAMS: usize = MIX + 4;
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
const LANE_AMOUNTS: [usize; LANES] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const LANE_AUX: [usize; LANES] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const DELAY_MODE: usize = AUX_BASE + 7;
const MODULATOR_MODE: usize = AUX_BASE + 10;
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
const MODULATOR_MODES: [&str; 3] = ["AM", "Ring", "FM"];

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
    ($(($prefix:literal, $label:literal, $amount:literal, $aux:literal, $amount_start:literal, $aux_start:literal, $aux_max:literal)),+ $(,)?) => {
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
                Setting { id: concat!($prefix, "_amount"), name: $amount, range: (0, 10000), start: $amount_start, magnet: None },
            )+
            $(
                Setting { id: concat!($prefix, "_aux"), name: $aux, range: (0, $aux_max), start: $aux_start, magnet: None },
            )+
            Setting { id: "grid_length", name: "Step count", range: (0, 1), start: 0, magnet: None },
            Setting { id: "bars", name: "Pattern bars", range: (0, 2), start: 0, magnet: None },
            Setting { id: "division", name: "Sub-division", range: (0, 3), start: 1, magnet: None },
            Setting { id: "mix", name: "Mix", range: (0, 10000), start: 10000, magnet: None },
            Setting { id: "delay_time_l", name: "Delay left", range: (0, 10000), start: 2500, magnet: None },
            Setting { id: "delay_time_r", name: "Delay right", range: (0, 10000), start: 2500, magnet: None },
            Setting { id: "delay_filter", name: "Delay feedback filter", range: (0, 10000), start: 8000, magnet: None },
        ]
    };
}

const SETTINGS: [Setting; PARAMS] = all_settings!(
    (
        "randomizer",
        "Randomizer",
        "Probability",
        "Seed",
        5000,
        4700,
        10000
    ),
    (
        "distortion",
        "Distortion",
        "Drive",
        "Tone",
        4000,
        7000,
        10000
    ),
    (
        "shuffler",
        "Shuffler",
        "Slice size",
        "Shuffle",
        5000,
        5000,
        10000
    ),
    (
        "stretcher",
        "Stretcher",
        "Stretch",
        "Grain size",
        5000,
        5000,
        10000
    ),
    (
        "reverser",
        "Reverser",
        "Reverse mix",
        "Crossfade",
        10000,
        3000,
        10000
    ),
    (
        "retrigger",
        "Retrigger",
        "Repeats",
        "Pitch slide",
        5000,
        0,
        10000
    ),
    ("gater", "Gater", "Duty", "Attack", 5000, 1000, 10000),
    ("delay", "Delay", "Feedback", "Ping-pong", 4000, 0, 1),
    (
        "lofi",
        "Lofi",
        "Bit depth",
        "Rate reduction",
        5000,
        4000,
        10000
    ),
    (
        "tape_stop",
        "Tape Stop",
        "Stop time",
        "Recovery",
        5000,
        5000,
        10000
    ),
    ("modulator", "Modulator", "Depth", "Mode", 5000, 0, 2),
);

const CARDS: [Card; 2] = [
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
        y0: 284.0,
        y1: 352.0,
        heading: "TRANSPORT",
    },
];

const CONTROLS: [Control; 8] = [
    Control::StepGrid {
        ids: &STEP_IDS,
        labels: &LANE_LABELS,
        enabled_ids: &ENABLE_IDS,
        amount_ids: &AMOUNT_IDS,
        aux_ids: &AUX_IDS,
        columns: MAX_STEPS,
        rows: LANES,
        length_id: "grid_length",
        rect: Rect::new(192.0, 42.0, 480.0, 220.0),
    },
    Control::Choice {
        id: "grid_length",
        x: 24.0,
        top: 306.0,
        names: &GRID_LENGTHS,
    },
    Control::Choice {
        id: "bars",
        x: 176.0,
        top: 306.0,
        names: &BAR_COUNTS,
    },
    Control::Choice {
        id: "division",
        x: 360.0,
        top: 306.0,
        names: &DIVISIONS,
    },
    Control::Knob {
        id: "mix",
        cx: 456.0,
        cy: 322.0,
        r: 14.0,
        caption: "MIX",
        bipolar: false,
    },
    Control::Knob {
        id: "delay_time_l",
        cx: 536.0,
        cy: 322.0,
        r: 14.0,
        caption: "DLY L",
        bipolar: false,
    },
    Control::Knob {
        id: "delay_time_r",
        cx: 616.0,
        cy: 322.0,
        r: 14.0,
        caption: "DLY R",
        bipolar: false,
    },
    Control::Knob {
        id: "delay_filter",
        cx: 696.0,
        cy: 322.0,
        r: 14.0,
        caption: "DLY TONE",
        bipolar: false,
    },
];

const LABELS: [(f32, f32, &'static str); 2] = [(690.0, 26.0, "AMT"), (729.0, 26.0, "AUX")];

fn text(k: usize, value: i32) -> String {
    match k {
        0..AMOUNT_BASE => if value > 0 { "On" } else { "Off" }.into(),
        DELAY_MODE => if value > 0 { "Ping-pong" } else { "Stereo" }.into(),
        MODULATOR_MODE => MODULATOR_MODES[value.clamp(0, 2) as usize].into(),
        GRID_LENGTH => GRID_LENGTHS[value.clamp(0, 1) as usize].into(),
        BARS => BAR_COUNTS[value.clamp(0, 2) as usize].into(),
        DIVISION => DIVISIONS[value.clamp(0, 3) as usize].into(),
        _ => format!("{:.1}%", value as f64 / 100.0),
    }
}

fn parse(k: usize, text: &str) -> Option<i32> {
    let text = text.trim().to_ascii_lowercase();
    match k {
        0..AMOUNT_BASE => match text.as_str() {
            "on" | "1" => Some(10000),
            "off" | "0" => Some(0),
            _ => None,
        },
        DELAY_MODE => ["stereo", "ping-pong"]
            .iter()
            .position(|name| *name == text)
            .map(|i| i as i32),
        MODULATOR_MODE => MODULATOR_MODES
            .iter()
            .position(|name| name.to_ascii_lowercase() == text)
            .map(|i| i as i32),
        GRID_LENGTH => GRID_LENGTHS
            .iter()
            .position(|name| name.to_ascii_lowercase() == text)
            .map(|i| i as i32),
        BARS => BAR_COUNTS
            .iter()
            .position(|name| name.to_ascii_lowercase() == text)
            .map(|i| i as i32),
        DIVISION => DIVISIONS
            .iter()
            .position(|name| name.to_ascii_lowercase() == text)
            .map(|i| i as i32),
        STEP_SETTINGS..=PARAMS => {
            let value = text.trim_end_matches('%').trim().parse::<f32>().ok()?;
            value
                .is_finite()
                .then_some((value * 100.0).round().clamp(0.0, 10000.0) as i32)
        }
        _ => None,
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
    size: (760, 370),
    cards: &CARDS,
    controls: &CONTROLS,
    display: None,
    mark: Mark {
        x: 744.0,
        y: 348.0,
        anchor: 1.0,
        stacked: false,
    },
    hint: None,
    labels: &LABELS,
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
        v[AMOUNT_BASE + GATER] = 10000;
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
}
