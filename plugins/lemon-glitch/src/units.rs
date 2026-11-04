//! How a stored setting (an int from 0 to the range's top) reads as a value with a unit. The editor's text and the
//! sound share these, so what a knob shows is what the effect does.

#[derive(Clone, Copy, Debug)]
pub enum Unit {
    /// 0 to 100 %.
    Percent,
    /// Linear from `lo` (stored 0) to `hi` (stored top).
    Lin {
        lo: f64,
        hi: f64,
        prec: usize,
        suffix: &'static str,
    },
    /// Exponential from `lo` to `hi`: equal steps are equal ratios (frequencies, times).
    Exp {
        lo: f64,
        hi: f64,
        prec: usize,
        suffix: &'static str,
    },
    /// Whole numbers from `lo`.
    Count { lo: i32, suffix: &'static str },
    /// A list of names, the stored value indexing it.
    Names(&'static [&'static str]),
    /// The stored value itself.
    Plain,
}

impl Unit {
    pub fn value(self, raw: i32, max: i32) -> f64 {
        let n = if max > 0 {
            (f64::from(raw) / f64::from(max)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        match self {
            Unit::Percent => n * 100.0,
            Unit::Lin { lo, hi, .. } => lo + (hi - lo) * n,
            Unit::Exp { lo, hi, .. } => lo * (hi / lo).powf(n),
            Unit::Count { lo, .. } => f64::from(lo + raw),
            Unit::Names(_) | Unit::Plain => f64::from(raw),
        }
    }

    pub fn show(self, raw: i32, max: i32) -> String {
        let v = self.value(raw, max);
        match self {
            Unit::Percent => format!("{v:.1}%"),
            Unit::Lin {
                lo, prec, suffix, ..
            } if lo < 0.0 => format!("{v:+.prec$}{suffix}"),
            Unit::Lin { prec, suffix, .. } | Unit::Exp { prec, suffix, .. } => {
                format!("{v:.prec$}{suffix}")
            }
            Unit::Count { suffix, .. } => format!("{v:.0}{suffix}"),
            Unit::Names(names) => names[raw.clamp(0, names.len() as i32 - 1) as usize].into(),
            Unit::Plain => format!("{raw}"),
        }
    }

    pub fn parse(self, text: &str, max: i32) -> Option<i32> {
        let text = text.trim().to_ascii_lowercase();
        if let Unit::Names(names) = self {
            return names
                .iter()
                .position(|name| name.to_ascii_lowercase() == text)
                .map(|i| i as i32);
        }

        let number: String = text
            .trim_start_matches(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '.')))
            .chars()
            .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'))
            .collect();
        let v: f64 = number.parse().ok().filter(|v: &f64| v.is_finite())?;
        let max_f = f64::from(max);
        let raw = match self {
            Unit::Percent => v / 100.0 * max_f,
            Unit::Lin { lo, hi, .. } => (v - lo) / (hi - lo) * max_f,
            Unit::Exp { lo, hi, .. } if v > 0.0 => (v / lo).ln() / (hi / lo).ln() * max_f,
            Unit::Exp { .. } => return None,
            Unit::Count { lo, .. } => v - f64::from(lo),
            Unit::Names(_) | Unit::Plain => v,
        };
        Some(raw.round().clamp(0.0, max_f) as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_round_trip_through_their_text() {
        let units = [
            Unit::Percent,
            Unit::Lin {
                lo: -12.0,
                hi: 12.0,
                prec: 2,
                suffix: " st",
            },
            Unit::Exp {
                lo: 0.1,
                hi: 2000.0,
                prec: 1,
                suffix: " Hz",
            },
            Unit::Count { lo: 1, suffix: "x" },
        ];
        for unit in units {
            let max = if matches!(unit, Unit::Count { .. }) {
                15
            } else {
                10000
            };
            for raw in [0, max / 3, max / 2, max] {
                let shown = unit.show(raw, max);
                let back = unit.parse(&shown, max).expect(&shown);
                let drift = (unit.value(back, max) - unit.value(raw, max)).abs();
                assert!(
                    back == raw || drift <= unit.value(raw, max).abs() * 0.02 + 0.05,
                    "{shown}: {raw} -> {back}"
                );
            }
        }
    }
}
