use super::*;
use std::f32::consts::TAU;

const HISTORY: usize = 1 << 19;
const MASK: usize = HISTORY - 1;
const MIN_READ: f64 = 1.0;
const MAX_READ: f64 = (HISTORY - 2) as f64;
/// How many candidate start positions the stretcher compares when it aligns a grain.
const SEARCH_CANDIDATES: f64 = 96.0;
/// Samples compared per candidate (every second sample of this many pairs).
const SEARCH_LENGTH: usize = 192;

#[derive(Clone, Copy, Debug)]
pub(super) struct HostPosition {
    pub beats: f64,
    pub bpm: f64,
}

fn splitmix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e3779b97f4a7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// Smooth 0..1 weight for a slice of `len` frames at `u`: it rises over `edge` frames at the start and falls over
/// `edge` at the end, so a jump of the read position never clicks.
fn edge_weight(u: f64, len: f64, edge: f64) -> f32 {
    let edge = edge.clamp(1.0, (len / 2.0).max(1.0));
    let x = (u.min(len - u) / edge).clamp(0.0, 1.0) as f32;
    x * x * (3.0 - 2.0 * x)
}

fn fold(v: f32) -> f32 {
    let p = (v + 1.0) * 0.25;
    4.0 * (p - (p + 0.5).floor()).abs() - 1.0
}

fn one_pole(hz: f64, rate: f64) -> f32 {
    (1.0 - (-f64::from(TAU) * hz / rate).exp()) as f32
}

fn semitones(st: f64) -> f64 {
    (st / 12.0).exp2()
}

/// One grain of the stretcher: where it reads (absolute sample position) and how far through its window it is.
#[derive(Clone, Copy, Default)]
struct Grain {
    pos: f64,
    phase: f32,
}

/// The settings read in their units, once per block.
struct Tune {
    change_cell: bool,
    random_chance: u64,
    random_seed: u64,
    drive: f32,
    drive_norm: f32,
    dist_type: i32,
    lowcut_k: f32,
    tone_k: f32,
    dist_level: f32,
    slices: usize,
    shuffle_blend: f32,
    shuffle_order: i32,
    shuffle_fade: f64,
    stretch_speed: f64,
    grain: f64,
    stretch_rho: f64,
    search: f64,
    rev_mix: f32,
    rev_fade: f64,
    rev_length: f64,
    rev_rho: f64,
    repeats: f64,
    retrig_pitch: f64,
    retrig_slide: f64,
    retrig_decay: f64,
    gate_duty: f64,
    gate_attack_k: f32,
    gate_release_k: f32,
    gate_depth: f32,
    gate_rate: f64,
    feedback: f32,
    ping_pong: bool,
    delay_scale_l: f64,
    delay_scale_r: f64,
    delay_k: f32,
    delay_level: f32,
    bits: f32,
    hold: f64,
    noise: f32,
    lofi_k: f32,
    stop_time: f64,
    recovery_frames: f64,
    curve: f64,
    final_speed: f32,
    depth: f32,
    mode: i32,
    mod_hz: f64,
    shape: i32,
    stereo: f64,
    mix: f32,
}

pub(super) struct GlitchDsp {
    sample_rate: u32,
    bpm: f64,
    free_beats: f64,
    last_cell: i64,
    last_step: i64,
    random_lane: Option<usize>,
    random_hit: bool,
    rng: u64,
    clock: u64,
    write: usize,
    history_l: Vec<f32>,
    history_r: Vec<f32>,
    echo_l: Vec<f32>,
    echo_r: Vec<f32>,
    on: [bool; LANES],
    fade: [f32; LANES],
    origin: [u64; LANES],
    low_l: f32,
    low_r: f32,
    tone_l: f32,
    tone_r: f32,
    perm: [u8; 16],
    perm_key: (i64, usize, i32, u64),
    grains: [Grain; 2],
    gate: f32,
    delay_filter_l: f32,
    delay_filter_r: f32,
    lofi_acc: f64,
    lofi_l: f32,
    lofi_r: f32,
    lofi_tone_l: f32,
    lofi_tone_r: f32,
    tape_delay: f64,
    tape_speed: f32,
    lfo_phase: f64,
    lfo_hold: f32,
    lfo_l: f32,
    lfo_r: f32,
    values: [i32; PARAMS],
}

impl GlitchDsp {
    pub fn new(sample_rate: u32, values: &[i32]) -> Self {
        let mut dsp = GlitchDsp {
            sample_rate: 44100,
            bpm: 120.0,
            free_beats: 0.0,
            last_cell: i64::MIN,
            last_step: i64::MIN,
            random_lane: None,
            random_hit: false,
            rng: 0x9e3779b97f4a7c15,
            clock: 0,
            write: 0,
            history_l: vec![0.0; HISTORY],
            history_r: vec![0.0; HISTORY],
            echo_l: vec![0.0; HISTORY],
            echo_r: vec![0.0; HISTORY],
            on: [false; LANES],
            fade: [0.0; LANES],
            origin: [0; LANES],
            low_l: 0.0,
            low_r: 0.0,
            tone_l: 0.0,
            tone_r: 0.0,
            perm: [0; 16],
            perm_key: (i64::MIN, 0, 0, 0),
            grains: [Grain::default(); 2],
            gate: 1.0,
            delay_filter_l: 0.0,
            delay_filter_r: 0.0,
            lofi_acc: 0.0,
            lofi_l: 0.0,
            lofi_r: 0.0,
            lofi_tone_l: 0.0,
            lofi_tone_r: 0.0,
            tape_delay: 1.0,
            tape_speed: 1.0,
            lfo_phase: 0.0,
            lfo_hold: 0.0,
            lfo_l: 0.0,
            lfo_r: 0.0,
            values: [0; PARAMS],
        };
        dsp.set_sample_rate(sample_rate);
        dsp.set(values);
        dsp
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.sample_rate = rate.max(1);
    }

    pub fn set(&mut self, values: &[i32]) {
        for (dst, src) in self.values.iter_mut().zip(values.iter().copied()) {
            *dst = src;
        }
    }

    pub fn reset(&mut self) {
        self.history_l.fill(0.0);
        self.history_r.fill(0.0);
        self.echo_l.fill(0.0);
        self.echo_r.fill(0.0);
        self.clock = 0;
        self.write = 0;
        self.free_beats = 0.0;
        self.last_cell = i64::MIN;
        self.last_step = i64::MIN;
        self.random_lane = None;
        self.random_hit = false;
        self.on = [false; LANES];
        self.fade = [0.0; LANES];
        self.origin = [0; LANES];
        self.low_l = 0.0;
        self.low_r = 0.0;
        self.tone_l = 0.0;
        self.tone_r = 0.0;
        self.perm_key = (i64::MIN, 0, 0, 0);
        self.grains = [Grain::default(); 2];
        self.gate = 1.0;
        self.delay_filter_l = 0.0;
        self.delay_filter_r = 0.0;
        self.lofi_acc = 0.0;
        self.lofi_l = 0.0;
        self.lofi_r = 0.0;
        self.lofi_tone_l = 0.0;
        self.lofi_tone_r = 0.0;
        self.tape_delay = 1.0;
        self.tape_speed = 1.0;
        self.lfo_phase = 0.0;
        self.lfo_hold = 0.0;
        self.lfo_l = 0.0;
        self.lfo_r = 0.0;
    }

    fn noise(&mut self) -> f32 {
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x.max(1);
        (self.rng >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }

    fn tune(&self, rate: f64) -> Tune {
        let v = |k: usize| value(&self.values, k);
        let norm = |k: usize| (f64::from(self.values[k]) / f64::from(top(k))).clamp(0.0, 1.0);
        let ms = |ms: f64| ms * 0.001 * rate;
        let drive = 10f64.powf(v(AMOUNT_BASE + DISTORTION) / 20.0) as f32;
        Tune {
            change_cell: self.values[RAND_CHANGE] > 0,
            random_chance: (norm(AMOUNT_BASE + RANDOMIZER) * 10000.0) as u64,
            random_seed: self.values[AUX_BASE + RANDOMIZER] as u64,
            drive,
            drive_norm: 1.0 / drive.tanh().max(1e-4),
            dist_type: self.values[DIST_TYPE],
            lowcut_k: one_pole(v(DIST_LOWCUT), rate),
            tone_k: one_pole(v(AUX_BASE + DISTORTION), rate),
            dist_level: 10f32.powf(v(DIST_LEVEL) as f32 / 20.0),
            slices: SLICE_VALUES[self.values[AMOUNT_BASE + SHUFFLER].clamp(0, 6) as usize],
            shuffle_blend: norm(AUX_BASE + SHUFFLER) as f32,
            shuffle_order: self.values[SHUF_ORDER],
            shuffle_fade: ms(v(SHUF_FADE)),
            stretch_speed: 1.0 / v(AMOUNT_BASE + STRETCHER).max(1.0),
            grain: ms(v(AUX_BASE + STRETCHER)),
            stretch_rho: semitones(v(STRETCH_PITCH)).clamp(0.5, 2.0),
            search: norm(STRETCH_SEARCH),
            rev_mix: norm(AMOUNT_BASE + REVERSER) as f32,
            rev_fade: ms(v(AUX_BASE + REVERSER)),
            rev_length: [0.125, 0.25, 0.5, 1.0][self.values[REV_LENGTH].clamp(0, 3) as usize],
            rev_rho: semitones(v(REV_PITCH)).clamp(0.5, 2.0),
            repeats: v(AMOUNT_BASE + RETRIGGER),
            retrig_pitch: v(AUX_BASE + RETRIGGER),
            retrig_slide: v(RETRIG_SLIDE),
            retrig_decay: 1.0 - norm(RETRIG_DECAY) * 0.6,
            gate_duty: 0.05 + 0.95 * norm(AMOUNT_BASE + GATER),
            gate_attack_k: 1.0 - (-1.0 / ms(v(AUX_BASE + GATER)).max(1.0) as f32).exp(),
            gate_release_k: 1.0 - (-1.0 / ms(v(GATE_RELEASE)).max(1.0) as f32).exp(),
            gate_depth: norm(GATE_DEPTH) as f32,
            gate_rate: [1.0, 2.0, 4.0][self.values[GATE_RATE].clamp(0, 2) as usize],
            feedback: norm(AMOUNT_BASE + DELAY) as f32 * 0.95,
            ping_pong: self.values[AUX_BASE + DELAY] > 0,
            delay_scale_l: v(DELAY_TIME_L),
            delay_scale_r: v(DELAY_TIME_R),
            delay_k: one_pole(v(DELAY_FILTER), rate),
            delay_level: norm(DELAY_LEVEL) as f32,
            bits: v(AMOUNT_BASE + LOFI) as f32,
            hold: v(AUX_BASE + LOFI).max(1.0),
            noise: norm(LOFI_NOISE) as f32,
            lofi_k: one_pole(v(LOFI_TONE), rate),
            stop_time: 0.1 + 0.9 * norm(AMOUNT_BASE + TAPE_STOP),
            recovery_frames: ms(v(AUX_BASE + TAPE_STOP)),
            curve: 0.5 * 8f64.powf(norm(TAPE_CURVE)),
            final_speed: norm(TAPE_FINAL) as f32,
            depth: norm(AMOUNT_BASE + MODULATOR) as f32,
            mode: self.values[AUX_BASE + MODULATOR].clamp(0, 2),
            mod_hz: v(MOD_RATE),
            shape: self.values[MOD_SHAPE].clamp(0, 3),
            stereo: v(MOD_STEREO) / 360.0,
            mix: norm(MIX) as f32,
        }
    }

    /// Reads a history buffer `delay` frames back (1 is the newest sample), blending between neighbours.
    fn history_sample(buffer: &[f32], write: usize, delay: f64) -> f32 {
        let delay = delay.clamp(MIN_READ, MAX_READ);
        let whole = delay.floor() as usize;
        let frac = (delay - whole as f64) as f32;
        let a = buffer[write.wrapping_sub(whole) & MASK];
        let b = buffer[write.wrapping_sub(whole + 1) & MASK];
        a + (b - a) * frac
    }

    fn read_history(&self, delay: f64) -> (f32, f32) {
        (
            Self::history_sample(&self.history_l, self.write, delay),
            Self::history_sample(&self.history_r, self.write, delay),
        )
    }

    /// Reads the history at an absolute sample position (the n-th sample written since the start).
    fn read_at(&self, pos: f64) -> (f32, f32) {
        self.read_history(self.clock as f64 - pos)
    }

    fn mono_at(&self, pos: f64) -> f32 {
        let (l, r) = self.read_at(pos);
        (l + r) * 0.5
    }

    /// Where the slices of a shuffled block go: slice `i` plays what was in slice `perm[i]` of the step before.
    fn permute(&mut self, block: i64, count: usize, order: i32, seed: u64) {
        let key = (block, count, order, seed);
        if self.perm_key == key {
            return;
        }
        self.perm_key = key;
        for i in 0..count {
            self.perm[i] = match order {
                0 => (i ^ 1).min(count - 1) as u8,
                1 => (count - 1 - i) as u8,
                2 => ((i + 1) % count) as u8,
                _ => i as u8,
            };
        }
        if order >= 3 {
            let mut state = splitmix(seed ^ splitmix(block as u64));
            for i in (1..count).rev() {
                state = splitmix(state);
                self.perm.swap(i, (state % (i as u64 + 1)) as usize);
            }
        }
    }

    /// Starts a grain over the audio the stretch has reached, nudged within the search range to the spot where it
    /// best continues the grain still sounding (a waveform-similarity overlap-add).
    fn start_grain(&mut self, index: usize, tune: &Tune) {
        let now = self.clock as f64;
        let rho = tune.stretch_rho;
        let grain = tune.grain;
        let elapsed = (self.clock - 1 - self.origin[STRETCHER]) as f64;
        let newest = now - 2.0 - (rho - 1.0).max(0.0) * grain;
        let oldest = (now - MAX_READ * 0.9 + rho * grain).min(newest);
        let nominal =
            (self.origin[STRETCHER] as f64 + tune.stretch_speed * elapsed).clamp(oldest, newest);
        let reach = tune.search * grain * 0.25;
        let mut best = nominal;
        if reach >= 2.0 {
            let lo = (nominal - reach).max(oldest);
            let hi = (nominal + reach).min(newest);
            let step = (reach / (SEARCH_CANDIDATES / 2.0)).max(1.0);
            let reference = self.grains[1 - index].pos;
            let mut best_score = f32::MIN;
            // Candidates sit on a grid through the nominal start, so the untouched position is always tried.
            let mut candidate = nominal - ((nominal - lo) / step).floor() * step;
            while candidate <= hi {
                let (mut dot, mut energy) = (0.0f32, 1e-9f32);
                for n in 0..SEARCH_LENGTH {
                    let offset = rho * (n * 2) as f64;
                    let y = self.mono_at(candidate + offset);
                    dot += self.mono_at(reference + offset) * y;
                    energy += y * y;
                }
                let closeness = 1.0 - 0.1 * ((candidate - nominal).abs() / reach) as f32;
                let score = dot / energy.sqrt() * closeness;
                if score > best_score {
                    best_score = score;
                    best = candidate;
                }
                candidate += step;
            }
        }
        self.grains[index].pos = best;
    }

    fn trigger(&mut self, lane: usize, dry: (f32, f32)) {
        self.origin[lane] = self.clock - 1;
        match lane {
            DISTORTION => {
                self.low_l = dry.0;
                self.low_r = dry.1;
                self.tone_l = dry.0;
                self.tone_r = dry.1;
            }
            STRETCHER => {
                let start = (self.clock - 1) as f64;
                self.grains = [
                    Grain {
                        pos: start,
                        phase: 0.0,
                    },
                    Grain {
                        pos: start,
                        phase: 0.5,
                    },
                ];
            }
            GATER => self.gate = 1.0,
            LOFI => {
                self.lofi_acc = f64::MAX;
                self.lofi_tone_l = dry.0;
                self.lofi_tone_r = dry.1;
            }
            TAPE_STOP => {
                self.tape_delay = 1.0;
                self.tape_speed = 1.0;
            }
            MODULATOR => {
                self.lfo_phase = 0.0;
                self.lfo_l = 0.0;
                self.lfo_r = 0.0;
            }
            _ => {}
        }
    }

    fn lfo(shape: i32, phase: f64) -> f32 {
        let p = phase.rem_euclid(1.0) as f32;
        match shape {
            0 => (TAU * p).sin(),
            1 => 1.0 - 4.0 * (p - 0.5).abs(),
            _ => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }

    pub(super) fn process_at(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        host: Option<HostPosition>,
    ) {
        let frames = left.len().min(right.len());
        let bpm = host
            .map(|p| p.bpm)
            .filter(|b| b.is_finite() && *b > 0.0)
            .unwrap_or(self.bpm);
        self.bpm = bpm;
        let rate = f64::from(self.sample_rate);
        let tune = self.tune(rate);
        let bars = [1.0, 2.0, 4.0][self.values[BARS].clamp(0, 2) as usize];
        let steps = if self.values[GRID_LENGTH] == 0 {
            16
        } else {
            32
        };
        let subdivisions = f64::from(1u32 << (self.values[DIVISION].clamp(0, 3) as u32 + 1));
        let pattern_beats = bars * 4.0;
        let beats_per_sample = bpm / (60.0 * rate);
        let step_beats = pattern_beats / steps as f64;
        let step_frames = (step_beats / beats_per_sample).max(2.0);
        let start_beats = host.map_or(self.free_beats, |p| p.beats);
        let fade_in = 1.0 / (0.002 * rate).max(1.0) as f32;
        let mut fade_out = [1.0 / (0.003 * rate).max(1.0) as f32; LANES];
        fade_out[STRETCHER] = 1.0 / (0.010 * rate).max(1.0) as f32;
        fade_out[TAPE_STOP] = 1.0 / tune.recovery_frames.max(1.0) as f32;

        for i in 0..frames {
            let beats = start_beats + i as f64 * beats_per_sample;
            let pattern_phase = beats.rem_euclid(pattern_beats) / pattern_beats;
            let step = ((pattern_phase * steps as f64).floor() as usize).min(steps - 1);
            let abs_step = (beats / step_beats).floor() as i64;
            let cell = (beats * subdivisions).floor() as i64;
            let (dry_l, dry_r) = (left[i], right[i]);
            let cur = self.write;
            self.history_l[cur] = dry_l;
            self.history_r[cur] = dry_r;
            self.write = (cur + 1) & MASK;
            self.clock += 1;

            let mut active = [false; LANES];
            for (lane, slot) in active.iter_mut().enumerate().skip(1) {
                *slot =
                    self.values[ENABLE_BASE + lane] > 0 && self.values[lane * MAX_STEPS + step] > 0;
            }

            // The randomizer plays one of the ten other effects, whatever their own steps say.
            let hit = self.values[ENABLE_BASE + RANDOMIZER] > 0
                && self.values[RANDOMIZER * MAX_STEPS + step] > 0;
            let reroll = if tune.change_cell {
                cell != self.last_cell
            } else {
                abs_step != self.last_step
            };
            if !hit {
                self.random_lane = None;
            } else if reroll || !self.random_hit {
                let key = if tune.change_cell { cell } else { abs_step } as u64;
                let h = splitmix(splitmix(tune.random_seed) ^ splitmix(key));
                self.random_lane = (h % 10000 < tune.random_chance)
                    .then(|| 1 + (splitmix(h) % (LANES as u64 - 1)) as usize);
            }
            self.random_hit = hit;
            self.last_step = abs_step;
            self.last_cell = cell;
            if let Some(lane) = self.random_lane {
                active[lane] = true;
            }

            for lane in 1..LANES {
                if active[lane] && !self.on[lane] {
                    self.trigger(lane, (dry_l, dry_r));
                }
                self.on[lane] = active[lane];
                let (target, speed) = if active[lane] {
                    (1.0, fade_in)
                } else {
                    (0.0, fade_out[lane])
                };
                self.fade[lane] += (target - self.fade[lane]).clamp(-speed, speed);
            }
            let running = |dsp: &Self, lane: usize| dsp.fade[lane] > 1e-4;
            let elapsed = |dsp: &Self, lane: usize| (dsp.clock - 1 - dsp.origin[lane]) as f64;

            let mut l = dry_l;
            let mut r = dry_r;

            if running(self, DISTORTION) {
                let f = self.fade[DISTORTION];
                self.low_l += tune.lowcut_k * (l - self.low_l);
                self.low_r += tune.lowcut_k * (r - self.low_r);
                let shape = |x: f32| match tune.dist_type {
                    1 => (x * tune.drive).clamp(-1.0, 1.0),
                    2 => fold(x * tune.drive),
                    _ => (x * tune.drive).tanh() * tune.drive_norm,
                };
                let (yl, yr) = (shape(l - self.low_l), shape(r - self.low_r));
                self.tone_l += tune.tone_k * (yl - self.tone_l);
                self.tone_r += tune.tone_k * (yr - self.tone_r);
                l += (self.tone_l * tune.dist_level - l) * f;
                r += (self.tone_r * tune.dist_level - r) * f;
            }

            if running(self, SHUFFLER) {
                let f = self.fade[SHUFFLER];
                let tau = elapsed(self, SHUFFLER);
                let count = tune.slices;
                let slice = step_frames / count as f64;
                let block = (tau / step_frames).floor();
                let within = tau - block * step_frames;
                let j = ((within / slice).floor() as usize).min(count - 1);
                let u = within - j as f64 * slice;
                self.permute(
                    block as i64,
                    count,
                    tune.shuffle_order,
                    tune.random_seed ^ 0x5bd1e995,
                );
                let source = f64::from(self.perm[j]);
                let delay = step_frames + (j as f64 - source) * slice + 1.0;
                let (sl, sr) = self.read_history(delay);
                let w = edge_weight(u, slice, tune.shuffle_fade) * tune.shuffle_blend * f;
                l += (sl - l) * w;
                r += (sr - r) * w;
            }

            if running(self, STRETCHER) {
                let f = self.fade[STRETCHER];
                let (mut sl, mut sr) = (0.0, 0.0);
                for index in 0..2 {
                    self.grains[index].phase += (1.0 / tune.grain.max(16.0)) as f32;
                    if self.grains[index].phase >= 1.0 {
                        self.grains[index].phase -= 1.0;
                        self.start_grain(index, &tune);
                    }
                    let grain = self.grains[index];
                    let window = 0.5 - 0.5 * (TAU * grain.phase).cos();
                    let (gl, gr) = self.read_at(grain.pos);
                    sl += gl * window;
                    sr += gr * window;
                    self.grains[index].pos += tune.stretch_rho;
                }
                l += (sl - l) * f;
                r += (sr - r) * f;
            }

            if running(self, REVERSER) {
                let f = self.fade[REVERSER];
                let tau = elapsed(self, REVERSER);
                let window = (step_frames * tune.rev_length).max(2.0);
                let u = tau - (tau / window).floor() * window;
                let (rl, rr) = self.read_history(1.0 + (1.0 + tune.rev_rho) * u);
                let w = edge_weight(u, window, tune.rev_fade) * tune.rev_mix * f;
                l += (rl - l) * w;
                r += (rr - r) * w;
            }

            if running(self, RETRIGGER) {
                let f = self.fade[RETRIGGER];
                let tau = elapsed(self, RETRIGGER);
                let slice = (step_frames / (tune.repeats + 1.0)).max(2.0);
                let repeat = (tau / slice).floor();
                if repeat >= 1.0 {
                    let u = tau - repeat * slice;
                    let rho =
                        semitones(tune.retrig_pitch + tune.retrig_slide * repeat).clamp(0.25, 2.0);
                    let (rl, rr) = self.read_history(1.0 + tau - rho * u);
                    let gain = tune.retrig_decay.powf(repeat - 1.0) as f32;
                    let w = edge_weight(u, slice, 0.002 * rate) * f;
                    l += (rl * gain - l) * w;
                    r += (rr * gain - r) * w;
                }
            }

            if running(self, GATER) {
                let f = self.fade[GATER];
                let phase = (beats * subdivisions * tune.gate_rate).rem_euclid(1.0);
                let target = if phase < tune.gate_duty {
                    1.0
                } else {
                    1.0 - tune.gate_depth
                };
                let k = if target > self.gate {
                    tune.gate_attack_k
                } else {
                    tune.gate_release_k
                };
                self.gate += (target - self.gate) * k;
                l += (l * self.gate - l) * f;
                r += (r * self.gate - r) * f;
            }

            // The delay keeps ringing after its step: the lane's fade only gates what feeds it.
            {
                let base = rate * 60.0 / bpm / subdivisions;
                let read = |scale: f64| (base * scale).clamp(2.0, MAX_READ) as usize;
                let el = self.echo_l[cur.wrapping_sub(read(tune.delay_scale_l)) & MASK];
                let er = self.echo_r[cur.wrapping_sub(read(tune.delay_scale_r)) & MASK];
                let (fl, fr) = if tune.ping_pong { (er, el) } else { (el, er) };
                self.delay_filter_l += tune.delay_k * (fl - self.delay_filter_l);
                self.delay_filter_r += tune.delay_k * (fr - self.delay_filter_r);
                if self.delay_filter_l.abs() < 1e-20 {
                    self.delay_filter_l = 0.0;
                }
                if self.delay_filter_r.abs() < 1e-20 {
                    self.delay_filter_r = 0.0;
                }
                let feed = self.fade[DELAY];
                self.echo_l[cur] = dry_l * feed + tune.feedback * self.delay_filter_l;
                self.echo_r[cur] = dry_r * feed + tune.feedback * self.delay_filter_r;
                l += self.delay_filter_l * tune.delay_level;
                r += self.delay_filter_r * tune.delay_level;
            }

            if running(self, LOFI) {
                let f = self.fade[LOFI];
                self.lofi_acc += 1.0;
                if self.lofi_acc >= tune.hold {
                    self.lofi_acc = 0.0;
                    let levels = 2f32.powf(tune.bits - 1.0);
                    let dither = tune.noise * 0.05;
                    let (nl, nr) = (self.noise() * dither, self.noise() * dither);
                    self.lofi_l = ((l + nl) * levels).round() / levels;
                    self.lofi_r = ((r + nr) * levels).round() / levels;
                }
                self.lofi_tone_l += tune.lofi_k * (self.lofi_l - self.lofi_tone_l);
                self.lofi_tone_r += tune.lofi_k * (self.lofi_r - self.lofi_tone_r);
                l += (self.lofi_tone_l - l) * f;
                r += (self.lofi_tone_r - r) * f;
            }

            if running(self, TAPE_STOP) {
                let f = self.fade[TAPE_STOP];
                if active[TAPE_STOP] {
                    let stop_frames = (step_frames * tune.stop_time).max(2.0);
                    let x = (elapsed(self, TAPE_STOP) / stop_frames).min(1.0);
                    self.tape_speed = tune.final_speed
                        + (1.0 - tune.final_speed) * (1.0 - x as f32).powf(tune.curve as f32);
                } else {
                    // Spinning back up while the wet signal fades out over the recovery time.
                    self.tape_speed += (1.0 - self.tape_speed)
                        * (6.0 / tune.recovery_frames.max(1.0)).min(1.0) as f32;
                }
                self.tape_delay =
                    (self.tape_delay + f64::from(1.0 - self.tape_speed)).clamp(1.0, MAX_READ * 0.9);
                let (tl, tr) = self.read_history(self.tape_delay);
                let volume = self.tape_speed.clamp(0.0, 1.0).sqrt();
                l += (tl * volume - l) * f;
                r += (tr * volume - r) * f;
            }

            if running(self, MODULATOR) {
                let f = self.fade[MODULATOR];
                let previous = self.lfo_phase;
                self.lfo_phase = (self.lfo_phase + tune.mod_hz / rate).fract();
                if self.lfo_phase < previous {
                    self.lfo_hold = self.noise();
                }
                let smooth = one_pole(1000.0, rate);
                let (target_l, target_r) = if tune.shape == 3 {
                    (self.lfo_hold, self.lfo_hold)
                } else {
                    (
                        Self::lfo(tune.shape, self.lfo_phase),
                        Self::lfo(tune.shape, self.lfo_phase + tune.stereo),
                    )
                };
                // Hard-edged shapes are slewed over about a millisecond so they do not click.
                self.lfo_l += smooth * (target_l - self.lfo_l);
                self.lfo_r += smooth * (target_r - self.lfo_r);
                let (ml, mr) = (self.lfo_l, self.lfo_r);
                let depth = tune.depth;
                let (wl, wr) = match tune.mode {
                    0 => (
                        l * (1.0 - depth + depth * (0.5 + 0.5 * ml)),
                        r * (1.0 - depth + depth * (0.5 + 0.5 * mr)),
                    ),
                    1 => (
                        l * (1.0 - depth + depth * ml),
                        r * (1.0 - depth + depth * mr),
                    ),
                    _ => {
                        let swing = f64::from(depth) * 0.008 * rate;
                        (
                            Self::history_sample(
                                &self.history_l,
                                self.write,
                                2.0 + swing * f64::from(0.5 + 0.5 * ml),
                            ),
                            Self::history_sample(
                                &self.history_r,
                                self.write,
                                2.0 + swing * f64::from(0.5 + 0.5 * mr),
                            ),
                        )
                    }
                };
                l += (wl - l) * f;
                r += (wr - r) * f;
            }

            left[i] = dry_l + (l - dry_l) * tune.mix;
            right[i] = dry_r + (r - dry_r) * tune.mix;
        }
        let end = start_beats + frames as f64 * beats_per_sample;
        self.free_beats = end.rem_euclid(pattern_beats);
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
