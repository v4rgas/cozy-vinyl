use std::f64::consts::PI;
pub const TAU: f64 = 2.0 * PI;
pub const T1: f64 = 3180e-6;
pub const T2: f64 = 318e-6;
pub const T3: f64 = 75e-6;

pub fn riaa_rec_gain(f: f64) -> f64 {
    let w = TAU * f;
    (1.0_f64).hypot(w * T1) * (1.0_f64).hypot(w * T3) / (1.0_f64).hypot(w * T2)
}
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..40 {
        term *= (x / (2 * k) as f64).powi(2);
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}
fn ksinc(t: f64, half: f64, cutoff: f64, beta: f64) -> f64 {
    let a = t / half;
    if a.abs() >= 1.0 {
        return 0.0;
    }
    let x = PI * cutoff * t;
    let s = if x.abs() < 1e-9 { 1.0 } else { x.sin() / x };
    cutoff * s * bessel_i0(beta * (1.0 - a * a).sqrt()) / bessel_i0(beta)
}
pub fn make_polyphase(taps: usize, phases: usize, cutoff: f64, beta: f64) -> Vec<f32> {
    let mut c = vec![0.0; (phases + 1) * taps];
    let half = taps as f64 / 2.0;
    for p in 0..=phases {
        let row: Vec<f64> = (0..taps)
            .map(|t| {
                ksinc(
                    t as f64 - half + 1.0 - p as f64 / phases as f64,
                    half,
                    cutoff,
                    beta,
                )
            })
            .collect();
        let sum: f64 = row.iter().sum();
        for t in 0..taps {
            c[p * taps + t] = (row[t] / sum) as f32;
        }
    }
    c
}
pub fn make_lowpass(taps: usize, cutoff: f64, beta: f64) -> Vec<f32> {
    let mid = (taps - 1) as f64 / 2.0;
    let mut h: Vec<f32> = (0..taps)
        .map(|t| ksinc(t as f64 - mid, mid + 1.0, cutoff, beta) as f32)
        .collect();
    // The original sums the stored Float32 taps in double precision.
    let sum: f64 = h.iter().map(|&tap| tap as f64).sum();
    for v in &mut h {
        *v = (*v as f64 / sum) as f32;
    }
    h
}
pub fn bilinear(b: [f64; 3], a: [f64; 3], fs: f64) -> [f64; 5] {
    let k = 2.0 * fs;
    let k2 = k * k;
    let a0 = a[0] + a[1] * k + a[2] * k2;
    [
        (b[0] + b[1] * k + b[2] * k2) / a0,
        (2.0 * b[0] - 2.0 * b[2] * k2) / a0,
        (b[0] - b[1] * k + b[2] * k2) / a0,
        (2.0 * a[0] - 2.0 * a[2] * k2) / a0,
        (a[0] - a[1] * k + a[2] * k2) / a0,
    ]
}
pub fn riaa_playback(fs: f64) -> [f64; 5] {
    let g = riaa_rec_gain(1000.0);
    bilinear([g, g * T2, 0.0], [1.0, T1 + T3, T1 * T3], fs)
}
pub fn one_pole(f: f64, fs: f64) -> f64 {
    1.0 - (-TAU * f / fs).exp()
}

// Groove selection also rounds negative revolution numbers; ties go towards +infinity
// as in the original Math.round, rather than Rust's away-from-zero convention.
pub(crate) fn nearest_integer(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// Mulberry32, including the same wrapping integer semantics as the JS master.
#[derive(Clone)]
pub struct Rng(u32);
impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }
    pub fn uniform(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        (t ^ (t >> 14)) as f64 / 4294967296.0
    }
    pub fn gauss(&mut self) -> f64 {
        (-2.0 * (1.0 - self.uniform()).ln()).sqrt() * (TAU * self.uniform()).cos()
    }
}
