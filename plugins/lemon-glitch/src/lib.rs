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

const STEPS: usize = 16;
const RETRIGGER: usize = STEPS;
const GATE: usize = STEPS + 1;
const CRUSH: usize = STEPS + 2;
const FILTER: usize = STEPS + 3;
const RETRIGGER_AMOUNT: usize = STEPS + 4;
const GATE_AMOUNT: usize = STEPS + 5;
const CRUSH_AMOUNT: usize = STEPS + 6;
const FILTER_AMOUNT: usize = STEPS + 7;
const MIX: usize = STEPS + 8;
const RATE: usize = STEPS + 9;
const PARAMS: usize = STEPS + 10;
const RATES: [&str; 4] = ["1x", "2x", "4x", "8x"];

const SETTINGS: [Setting; PARAMS] = [
    Setting {
        id: "step_01",
        name: "Step 1",
        range: (0, 10000),
        start: 10000,
        magnet: None,
    },
    Setting {
        id: "step_02",
        name: "Step 2",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_03",
        name: "Step 3",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_04",
        name: "Step 4",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_05",
        name: "Step 5",
        range: (0, 10000),
        start: 10000,
        magnet: None,
    },
    Setting {
        id: "step_06",
        name: "Step 6",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_07",
        name: "Step 7",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_08",
        name: "Step 8",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_09",
        name: "Step 9",
        range: (0, 10000),
        start: 10000,
        magnet: None,
    },
    Setting {
        id: "step_10",
        name: "Step 10",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_11",
        name: "Step 11",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_12",
        name: "Step 12",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_13",
        name: "Step 13",
        range: (0, 10000),
        start: 10000,
        magnet: None,
    },
    Setting {
        id: "step_14",
        name: "Step 14",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_15",
        name: "Step 15",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "step_16",
        name: "Step 16",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "retrigger_lane",
        name: "Retrigger lane",
        range: (0, 10000),
        start: 10000,
        magnet: None,
    },
    Setting {
        id: "gate_lane",
        name: "Gate lane",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "crush_lane",
        name: "Crush lane",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "filter_lane",
        name: "Filter lane",
        range: (0, 10000),
        start: 0,
        magnet: None,
    },
    Setting {
        id: "retrigger_amount",
        name: "Retrigger depth",
        range: (0, 10000),
        start: 6500,
        magnet: None,
    },
    Setting {
        id: "gate_amount",
        name: "Gate depth",
        range: (0, 10000),
        start: 6500,
        magnet: None,
    },
    Setting {
        id: "crush_amount",
        name: "Crush depth",
        range: (0, 10000),
        start: 6500,
        magnet: None,
    },
    Setting {
        id: "filter_amount",
        name: "Filter depth",
        range: (0, 10000),
        start: 6500,
        magnet: None,
    },
    Setting {
        id: "mix",
        name: "Mix",
        range: (0, 10000),
        start: 10000,
        magnet: None,
    },
    Setting {
        id: "rate",
        name: "Subdivision",
        range: (0, 3),
        start: 2,
        magnet: None,
    },
];

const CARDS: [Card; 6] = [
    Card {
        x0: 8.0,
        x1: 752.0,
        y0: 26.0,
        y1: 96.0,
        heading: "PATTERN",
    },
    Card {
        x0: 8.0,
        x1: 188.0,
        y0: 104.0,
        y1: 248.0,
        heading: "RETRIGGER",
    },
    Card {
        x0: 194.0,
        x1: 374.0,
        y0: 104.0,
        y1: 248.0,
        heading: "GATE",
    },
    Card {
        x0: 380.0,
        x1: 560.0,
        y0: 104.0,
        y1: 248.0,
        heading: "CRUSH",
    },
    Card {
        x0: 566.0,
        x1: 752.0,
        y0: 104.0,
        y1: 248.0,
        heading: "FILTER",
    },
    Card {
        x0: 8.0,
        x1: 752.0,
        y0: 256.0,
        y1: 318.0,
        heading: "OUTPUT",
    },
];

const CONTROLS: [Control; PARAMS] = [
    Control::Switch {
        id: "step_01",
        cx: 30.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_02",
        cx: 76.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_03",
        cx: 122.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_04",
        cx: 168.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_05",
        cx: 214.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_06",
        cx: 260.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_07",
        cx: 306.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_08",
        cx: 352.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_09",
        cx: 398.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_10",
        cx: 444.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_11",
        cx: 490.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_12",
        cx: 536.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_13",
        cx: 582.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_14",
        cx: 628.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_15",
        cx: 674.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "step_16",
        cx: 720.0,
        cy: 68.0,
    },
    Control::Switch {
        id: "retrigger_lane",
        cx: 32.0,
        cy: 166.0,
    },
    Control::Switch {
        id: "gate_lane",
        cx: 218.0,
        cy: 166.0,
    },
    Control::Switch {
        id: "crush_lane",
        cx: 404.0,
        cy: 166.0,
    },
    Control::Switch {
        id: "filter_lane",
        cx: 590.0,
        cy: 166.0,
    },
    Control::Knob {
        id: "retrigger_amount",
        cx: 126.0,
        cy: 174.0,
        r: 19.0,
        caption: "DEPTH",
        bipolar: false,
    },
    Control::Knob {
        id: "gate_amount",
        cx: 312.0,
        cy: 174.0,
        r: 19.0,
        caption: "DEPTH",
        bipolar: false,
    },
    Control::Knob {
        id: "crush_amount",
        cx: 498.0,
        cy: 174.0,
        r: 19.0,
        caption: "DEPTH",
        bipolar: false,
    },
    Control::Knob {
        id: "filter_amount",
        cx: 684.0,
        cy: 174.0,
        r: 19.0,
        caption: "DEPTH",
        bipolar: false,
    },
    Control::Knob {
        id: "mix",
        cx: 266.0,
        cy: 288.0,
        r: 19.0,
        caption: "MIX",
        bipolar: false,
    },
    Control::Choice {
        id: "rate",
        x: 24.0,
        top: 276.0,
        names: &RATES,
    },
];

const LABELS: [(f32, f32, &'static str); STEPS] = [
    (26.0, 40.0, "1"),
    (72.0, 40.0, "2"),
    (118.0, 40.0, "3"),
    (164.0, 40.0, "4"),
    (210.0, 40.0, "5"),
    (256.0, 40.0, "6"),
    (302.0, 40.0, "7"),
    (348.0, 40.0, "8"),
    (394.0, 40.0, "9"),
    (440.0, 40.0, "10"),
    (486.0, 40.0, "11"),
    (532.0, 40.0, "12"),
    (578.0, 40.0, "13"),
    (624.0, 40.0, "14"),
    (670.0, 40.0, "15"),
    (716.0, 40.0, "16"),
];

fn text(k: usize, value: i32) -> String {
    match k {
        0..=FILTER => if value > 0 { "On" } else { "Off" }.into(),
        RETRIGGER_AMOUNT..=MIX => format!("{:.1}%", value as f64 / 100.0),
        _ => RATES[value.clamp(0, 3) as usize].into(),
    }
}

fn parse(k: usize, text: &str) -> Option<i32> {
    let text = text.trim().to_ascii_lowercase();
    match k {
        0..=FILTER => match text.as_str() {
            "on" | "1" => Some(10000),
            "off" | "0" => Some(0),
            _ => None,
        },
        RATE => RATES
            .iter()
            .position(|name| name.to_ascii_lowercase() == text)
            .map(|i| i as i32),
        RETRIGGER_AMOUNT..=MIX => {
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

#[derive(Clone, Copy, Debug)]
struct HostPosition {
    beats: f64,
    bpm: f64,
}

struct GlitchDsp {
    sample_rate: u32,
    bpm: f64,
    phase_beats: f64,
    last_cell: i32,
    hold_l: f32,
    hold_r: f32,
    filter_l: f32,
    filter_r: f32,
    gate: f32,
    gate_k: f32,
    values: [i32; PARAMS],
}

impl GlitchDsp {
    fn new(sample_rate: u32, values: &[i32]) -> Self {
        let mut dsp = GlitchDsp {
            sample_rate: 44100,
            bpm: 120.0,
            phase_beats: 0.0,
            last_cell: -1,
            hold_l: 0.0,
            hold_r: 0.0,
            filter_l: 0.0,
            filter_r: 0.0,
            gate: 1.0,
            gate_k: 0.0,
            values: [0; PARAMS],
        };
        dsp.set_sample_rate(sample_rate);
        dsp.set(values);
        dsp
    }

    fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
        self.gate_k = 1.0 - f32::exp(-1.0 / (0.001 * self.sample_rate as f32));
    }

    fn set(&mut self, values: &[i32]) {
        for (dst, src) in self.values.iter_mut().zip(values.iter().copied()) {
            *dst = src;
        }
    }

    fn reset(&mut self) {
        self.phase_beats = 0.0;
        self.last_cell = -1;
        self.hold_l = 0.0;
        self.hold_r = 0.0;
        self.filter_l = 0.0;
        self.filter_r = 0.0;
        self.gate = 1.0;
    }

    fn process_at(&mut self, left: &mut [f32], right: &mut [f32], host: Option<HostPosition>) {
        let frames = left.len().min(right.len());
        let bpm = host
            .map(|position| position.bpm)
            .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
            .unwrap_or(self.bpm);
        self.bpm = bpm;
        let beats_per_sample = bpm / (60.0 * f64::from(self.sample_rate));
        let subdivisions = 1usize << self.values[RATE].clamp(0, 3);
        let mix = (self.values[MIX] as f32 / 10000.0).clamp(0.0, 1.0);
        let lane_on = [RETRIGGER, GATE, CRUSH, FILTER].map(|index| self.values[index] > 0);
        let amount = [RETRIGGER_AMOUNT, GATE_AMOUNT, CRUSH_AMOUNT, FILTER_AMOUNT]
            .map(|index| (self.values[index] as f32 / 10000.0).clamp(0.0, 1.0));
        let start_beats = host.map_or(self.phase_beats, |position| position.beats);

        for i in 0..frames {
            let beats = start_beats + i as f64 * beats_per_sample;
            let bar_phase = beats.rem_euclid(4.0) * 4.0;
            let step = bar_phase.floor().clamp(0.0, 15.0) as usize;
            let within = (bar_phase - step as f64).clamp(0.0, 0.999_999) as f32;
            let sub = (within * subdivisions as f32).floor() as usize;
            let sub_phase = within * subdivisions as f32 - sub as f32;
            let cell = (step * subdivisions + sub) as i32;
            let (dry_l, dry_r) = (left[i], right[i]);
            if cell != self.last_cell {
                self.hold_l = dry_l;
                self.hold_r = dry_r;
                self.last_cell = cell;
            }
            let enabled = self.values[step] > 0;
            let (mut wet_l, mut wet_r) = (dry_l, dry_r);

            if enabled {
                if lane_on[0] {
                    wet_l += (self.hold_l - wet_l) * amount[0];
                    wet_r += (self.hold_r - wet_r) * amount[0];
                }
                if lane_on[1] {
                    let duty = 1.0 - amount[1] * 0.9;
                    let gate_target = f32::from(sub_phase < duty);
                    self.gate += (gate_target - self.gate) * self.gate_k;
                    wet_l *= self.gate;
                    wet_r *= self.gate;
                } else {
                    self.gate += (1.0 - self.gate) * self.gate_k;
                }
                if lane_on[2] {
                    let bits = 16.0 - amount[2] * 12.0;
                    let levels = 2.0_f32.powf(bits - 1.0);
                    wet_l = (wet_l * levels).round() / levels;
                    wet_r = (wet_r * levels).round() / levels;
                }
                if lane_on[3] {
                    let cutoff = 250.0 + (1.0 - amount[3]).powi(2) * 11750.0;
                    let coefficient =
                        1.0 - f32::exp(-std::f32::consts::TAU * cutoff / self.sample_rate as f32);
                    self.filter_l += coefficient * (wet_l - self.filter_l);
                    self.filter_r += coefficient * (wet_r - self.filter_r);
                    wet_l = self.filter_l;
                    wet_r = self.filter_r;
                }
            } else {
                self.gate += (1.0 - self.gate) * self.gate_k;
            }

            left[i] = dry_l + (wet_l - dry_l) * mix;
            right[i] = dry_r + (wet_r - dry_r) * mix;
        }
        self.phase_beats = (start_beats + frames as f64 * beats_per_sample).rem_euclid(4.0);
    }
}

impl Sound for GlitchDsp {
    fn set(&mut self, values: &[i32]) {
        GlitchDsp::set(self, values);
    }
    fn set_sample_rate(&mut self, rate: u32) {
        GlitchDsp::set_sample_rate(self, rate);
    }
    fn reset(&mut self) {
        GlitchDsp::reset(self);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_at(left, right, None);
    }
}

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

    fn values(mix: i32) -> [i32; PARAMS] {
        let mut v = std::array::from_fn(|i| SETTINGS[i].start);
        v[MIX] = mix;
        v
    }

    #[test]
    fn disabled_steps_are_bit_exact_passthrough() {
        let mut v = values(10000);
        v[..STEPS].fill(0);
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
        v[RETRIGGER] = 10000;
        v[CRUSH] = 10000;
        v[RETRIGGER_AMOUNT] = 10000;
        v[CRUSH_AMOUNT] = 10000;
        let mut dsp = GlitchDsp::new(48000, &v);
        let mut left = vec![0.9, 0.1, -0.8, 0.2];
        let mut right = vec![-0.7, 0.3, 0.8, -0.2];
        dsp.process_at(
            &mut left,
            &mut right,
            Some(HostPosition {
                beats: 0.0,
                bpm: 120.0,
            }),
        );
        assert!(
            left[1..]
                .iter()
                .all(|sample| (*sample - left[0]).abs() < 1e-6),
            "left: {left:?}"
        );
        assert!(
            right[1..]
                .iter()
                .all(|sample| (*sample - right[0]).abs() < 1e-6),
            "right: {right:?}"
        );
    }

    #[test]
    fn step_pattern_tracks_host_bar_position() {
        let mut v = values(10000);
        v[..STEPS].fill(0);
        v[4] = 10000;
        v[RETRIGGER] = 0;
        v[GATE] = 10000;
        v[GATE_AMOUNT] = 10000;
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
            left[512..].iter().all(|sample| *sample < 1e-4),
            "step 5 gates the off portion of its subdivision"
        );
    }
}
