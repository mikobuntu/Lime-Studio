use super::*;

const HISTORY: usize = 1 << 19;
const MASK: usize = HISTORY - 1;
const MIN_READ: f64 = 1.0;
const MAX_READ: f64 = (HISTORY - 2) as f64;
const CROSSFADE_FRAMES: f64 = 32.0;

#[derive(Clone, Copy, Debug)]
pub(super) struct HostPosition {
    pub beats: f64,
    pub bpm: f64,
}

pub(super) struct GlitchDsp {
    sample_rate: u32,
    bpm: f64,
    free_beats: f64,
    last_cell: i64,
    last_step: usize,
    random_lane: Option<usize>,
    rng: u64,
    write: usize,
    history_l: Vec<f32>,
    history_r: Vec<f32>,
    echo_l: Vec<f32>,
    echo_r: Vec<f32>,
    hold_l: f32,
    hold_r: f32,
    distortion_l: f32,
    distortion_r: f32,
    delay_filter_l: f32,
    delay_filter_r: f32,
    grain_phase: f64,
    lfo_phase: f64,
    tape_progress: f32,
    gate: f32,
    lofi_l: f32,
    lofi_r: f32,
    lofi_counter: usize,
    values: [i32; PARAMS],
}

impl GlitchDsp {
    pub fn new(sample_rate: u32, values: &[i32]) -> Self {
        let mut dsp = GlitchDsp {
            sample_rate: 44100,
            bpm: 120.0,
            free_beats: 0.0,
            last_cell: i64::MIN,
            last_step: usize::MAX,
            random_lane: None,
            rng: 0x9e3779b97f4a7c15,
            write: 0,
            history_l: vec![0.0; HISTORY],
            history_r: vec![0.0; HISTORY],
            echo_l: vec![0.0; HISTORY],
            echo_r: vec![0.0; HISTORY],
            hold_l: 0.0,
            hold_r: 0.0,
            distortion_l: 0.0,
            distortion_r: 0.0,
            delay_filter_l: 0.0,
            delay_filter_r: 0.0,
            grain_phase: 0.0,
            lfo_phase: 0.0,
            tape_progress: 0.0,
            gate: 1.0,
            lofi_l: 0.0,
            lofi_r: 0.0,
            lofi_counter: 0,
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
        self.write = 0;
        self.free_beats = 0.0;
        self.last_cell = i64::MIN;
        self.last_step = usize::MAX;
        self.random_lane = None;
        self.hold_l = 0.0;
        self.hold_r = 0.0;
        self.distortion_l = 0.0;
        self.distortion_r = 0.0;
        self.delay_filter_l = 0.0;
        self.delay_filter_r = 0.0;
        self.grain_phase = 0.0;
        self.lfo_phase = 0.0;
        self.tape_progress = 0.0;
        self.gate = 1.0;
        self.lofi_l = 0.0;
        self.lofi_r = 0.0;
        self.lofi_counter = 0;
    }

    fn random(&mut self) -> u64 {
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x.max(1);
        self.rng
    }

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

    fn edge_fade(phase: f32, frames: f64) -> f32 {
        let width = (CROSSFADE_FRAMES / frames.max(CROSSFADE_FRAMES)).clamp(0.01, 0.5) as f32;
        let edge = (phase.min(1.0 - phase) / width).clamp(0.0, 1.0);
        edge * edge * (3.0 - 2.0 * edge)
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
        let bars = [1.0, 2.0, 4.0][self.values[BARS].clamp(0, 2) as usize];
        let steps = if self.values[GRID_LENGTH] == 0 {
            16
        } else {
            32
        };
        let subdivisions = 1usize << (self.values[DIVISION].clamp(0, 3) as usize + 1);
        let pattern_beats = bars * 4.0;
        let beats_per_sample = bpm / (60.0 * f64::from(self.sample_rate));
        let step_beats = pattern_beats / steps as f64;
        let step_frames = (step_beats / beats_per_sample).max(2.0);
        let start_beats = host.map_or(self.free_beats, |p| p.beats);
        let mix = self.values[MIX].clamp(0, 10000) as f32 / 10000.0;

        for i in 0..frames {
            let beats = start_beats + i as f64 * beats_per_sample;
            let pattern_phase = beats.rem_euclid(pattern_beats) / pattern_beats;
            let step_pos = pattern_phase * steps as f64;
            let step = (step_pos.floor() as usize).min(steps - 1);
            let phase = (step_pos - step as f64) as f32;
            if step != self.last_step {
                self.last_step = step;
                self.tape_progress = 0.0;
            }
            let sub_phase = (beats * subdivisions as f64).fract() as f32;
            let cell = (beats * subdivisions as f64).floor() as i64;
            let (dry_l, dry_r) = (left[i], right[i]);
            let write = self.write;
            self.history_l[write] = dry_l;
            self.history_r[write] = dry_r;
            self.write = (write + 1) & MASK;

            let mut active = [false; LANES];
            for lane in 1..LANES {
                active[lane] =
                    self.values[ENABLE_BASE + lane] > 0 && self.values[lane * MAX_STEPS + step] > 0;
            }
            let randomizer_hit = self.values[ENABLE_BASE + RANDOMIZER] > 0
                && self.values[RANDOMIZER * MAX_STEPS + step] > 0;
            if cell != self.last_cell {
                self.last_cell = cell;
                self.rng ^= (self.values[AUX_BASE + RANDOMIZER] as u64 + 1)
                    .wrapping_mul(cell.unsigned_abs() + 1);
                self.random_lane = None;
                if randomizer_hit
                    && self.random() % 10000
                        < self.values[AMOUNT_BASE + RANDOMIZER].clamp(0, 10000) as u64
                {
                    let mut candidates = [0usize; LANES - 1];
                    let mut count = 0;
                    for lane in 1..LANES {
                        if active[lane] {
                            candidates[count] = lane;
                            count += 1;
                        }
                    }
                    if count > 0 {
                        self.random_lane = Some(candidates[self.random() as usize % count]);
                    }
                }
            }
            if let Some(lane) = self.random_lane {
                active[lane] = true;
            }
            let amount = |lane: usize| {
                self.values[AMOUNT_BASE + LANE_AMOUNTS[lane]].clamp(0, 10000) as f32 / 10000.0
            };
            let aux = |lane: usize| {
                self.values[AUX_BASE + LANE_AUX[lane]].clamp(0, 10000) as f32 / 10000.0
            };
            let mut l = dry_l;
            let mut r = dry_r;

            if active[DISTORTION] {
                let cutoff = 300.0 + aux(DISTORTION) * 17000.0;
                let k = 1.0 - f32::exp(-std::f32::consts::TAU * cutoff / self.sample_rate as f32);
                self.distortion_l += k * (l - self.distortion_l);
                self.distortion_r += k * (r - self.distortion_r);
                let drive = 1.0 + amount(DISTORTION) * 28.0;
                let norm = 1.0 / drive.tanh().max(1e-4);
                l = (self.distortion_l * drive).tanh() * norm;
                r = (self.distortion_r * drive).tanh() * norm;
            }
            if active[SHUFFLER] {
                let chunks = ((2.0 + amount(SHUFFLER) * 14.0) / 2.0).round() * 2.0;
                let chunk = (phase * chunks).floor();
                let local = (phase * chunks - chunk).clamp(0.0, 1.0);
                let shuffled = if chunk as i32 % 2 == 0 {
                    chunk + 1.0
                } else {
                    chunk - 1.0
                };
                let source = ((shuffled + local) / chunks).clamp(0.0, 1.0);
                let (sl, sr) = self.read_history(1.0 + step_frames * (1.0 - source as f64));
                let blend = amount(SHUFFLER)
                    * (0.5 + 0.5 * aux(SHUFFLER))
                    * Self::edge_fade(local, step_frames / chunks as f64);
                l += (sl - l) * blend;
                r += (sr - r) * blend;
            }
            if active[STRETCHER] {
                let grain = 128.0 + aux(STRETCHER) * 4096.0;
                self.grain_phase = (self.grain_phase
                    + f64::from(1.0 + amount(STRETCHER) * 3.0) / f64::from(grain))
                .fract();
                let p = self.grain_phase as f32;
                let p2 = (p + 0.5).fract();
                let w1 = 0.5 - 0.5 * f32::cos(std::f32::consts::TAU * p);
                let w2 = 0.5 - 0.5 * f32::cos(std::f32::consts::TAU * p2);
                let (a_l, a_r) = self.read_history(1.0 + f64::from(grain * p));
                let (b_l, b_r) = self.read_history(1.0 + f64::from(grain * p2));
                let weight = (w1 + w2).max(0.01);
                let sl = (a_l * w1 + b_l * w2) / weight;
                let sr = (a_r * w1 + b_r * w2) / weight;
                let blend = amount(STRETCHER) * Self::edge_fade(p, f64::from(grain));
                l += (sl - l) * blend;
                r += (sr - r) * blend;
            }
            if active[REVERSER] {
                let (rl, rr) = self.read_history(1.0 + step_frames * phase as f64);
                let edge = Self::edge_fade(phase, step_frames) * (1.0 - aux(REVERSER) * 0.5);
                let blend = amount(REVERSER) * edge;
                l += (rl - l) * blend;
                r += (rr - r) * blend;
            }
            if active[RETRIGGER] {
                let repeats = 1.0 + amount(RETRIGGER) * 15.0;
                let p = (phase * repeats).fract();
                let pitch = 1.0 + (aux(RETRIGGER) - 0.5) * 2.0;
                let source = (p as f64 * pitch as f64).rem_euclid(1.0);
                let (rl, rr) = self.read_history(1.0 + step_frames * (1.0 - source));
                let blend = amount(RETRIGGER) * Self::edge_fade(p, step_frames / repeats as f64);
                l += (rl - l) * blend;
                r += (rr - r) * blend;
            }
            if active[GATER] {
                let target = f32::from(sub_phase < 1.0 - amount(GATER) * 0.95);
                let attack = 0.0001 + aux(GATER) * 0.02;
                let k = 1.0 - f32::exp(-1.0 / (attack * self.sample_rate as f32));
                self.gate += (target - self.gate) * k;
                l *= self.gate;
                r *= self.gate;
            } else {
                self.gate +=
                    (1.0 - self.gate) * (1.0 - f32::exp(-1.0 / (0.001 * self.sample_rate as f32)));
            }
            if active[DELAY] {
                let base_beats = 1.0 / subdivisions as f64;
                let scale_l =
                    0.25 + 3.75 * self.values[DELAY_TIME_L].clamp(0, 10000) as f64 / 10000.0;
                let scale_r =
                    0.25 + 3.75 * self.values[DELAY_TIME_R].clamp(0, 10000) as f64 / 10000.0;
                let frames_per_beat = self.sample_rate as f64 * 60.0 / bpm;
                let read_l = write.wrapping_sub(
                    (base_beats * scale_l * frames_per_beat).clamp(2.0, MAX_READ) as usize,
                ) & MASK;
                let read_r = write.wrapping_sub(
                    (base_beats * scale_r * frames_per_beat).clamp(2.0, MAX_READ) as usize,
                ) & MASK;
                let (el, er) = (self.echo_l[read_l], self.echo_r[read_r]);
                let feedback = amount(DELAY) * 0.94;
                let ping = self.values[AUX_BASE + DELAY] > 0;
                let cutoff = 250.0 + self.values[DELAY_FILTER] as f32 / 10000.0 * 17750.0;
                let filter_k =
                    1.0 - f32::exp(-std::f32::consts::TAU * cutoff / self.sample_rate as f32);
                self.delay_filter_l +=
                    filter_k * ((if ping { er } else { el }) - self.delay_filter_l);
                self.delay_filter_r +=
                    filter_k * ((if ping { el } else { er }) - self.delay_filter_r);
                self.echo_l[write] = dry_l + feedback * self.delay_filter_l;
                self.echo_r[write] = dry_r + feedback * self.delay_filter_r;
                l += self.delay_filter_l;
                r += self.delay_filter_r;
            } else {
                self.echo_l[write] = dry_l;
                self.echo_r[write] = dry_r;
            }
            if active[LOFI] {
                let hold = 1 + (aux(LOFI) * 31.0) as usize;
                if self.lofi_counter == 0 {
                    let bits = 16.0 - amount(LOFI) * 13.0;
                    let levels = 2.0_f32.powf(bits - 1.0);
                    self.lofi_l = (l * levels).round() / levels;
                    self.lofi_r = (r * levels).round() / levels;
                }
                self.lofi_counter = (self.lofi_counter + 1) % hold;
                l = self.lofi_l;
                r = self.lofi_r;
            } else {
                self.lofi_counter = 0;
            }
            if active[TAPE_STOP] {
                let speed = (1.0 - amount(TAPE_STOP) * self.tape_progress).powi(2);
                self.tape_progress =
                    (self.tape_progress + speed / step_frames as f32).clamp(0.0, 1.0);
                let (tl, tr) =
                    self.read_history(1.0 + step_frames * (1.0 - self.tape_progress as f64));
                let fade = Self::edge_fade(phase, step_frames) * (0.5 + 0.5 * aux(TAPE_STOP));
                let blend = amount(TAPE_STOP) * (0.5 + 0.5 * fade);
                l += (tl - l) * blend;
                r += (tr - r) * blend;
            }
            if active[MODULATOR] {
                let hz = 0.2 + amount(MODULATOR) * 19.8;
                self.lfo_phase = (self.lfo_phase + hz as f64 / self.sample_rate as f64).fract();
                let phase = self.lfo_phase as f32 * std::f32::consts::TAU;
                let depth = amount(MODULATOR);
                match self.values[AUX_BASE + MODULATOR].clamp(0, 2) {
                    0 => {
                        let m = 1.0 - depth + depth * (0.5 + 0.5 * phase.sin());
                        l *= m;
                        r *= m;
                    }
                    1 => {
                        let m = 1.0 - depth + depth * phase.sin();
                        l *= m;
                        r *= m;
                    }
                    _ => {
                        l = (phase + l * depth * 6.0).sin() * depth + l * (1.0 - depth);
                        r = (phase + r * depth * 6.0).sin() * depth + r * (1.0 - depth);
                    }
                }
            }
            left[i] = dry_l + (l - dry_l) * mix;
            right[i] = dry_r + (r - dry_r) * mix;
        }
        self.free_beats =
            (start_beats + frames as f64 * beats_per_sample).rem_euclid(pattern_beats);
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
