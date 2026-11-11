//! The original synthesized EP: electric piano groove, measurement band and arpeggios.
use crate::{
    cut::AudioTrack,
    dsp::{Rng, TAU},
};
use std::f64::consts::PI;
struct Buffer {
    left: Vec<f32>,
    right: Vec<f32>,
    fs: f64,
}
impl Buffer {
    fn new(fs: f64, seconds: f64) -> Self {
        let n = (fs * seconds).round() as usize;
        Self {
            left: vec![0.0; n],
            right: vec![0.0; n],
            fs,
        }
    }
}
fn hz(note: f64) -> f64 {
    440.0 * 2.0_f64.powf((note - 69.0) / 12.0)
}
fn voice(
    b: &mut Buffer,
    t0: f64,
    dur: f64,
    tail: f64,
    pan: f64,
    mut sample: impl FnMut(f64) -> f64,
) {
    let i0 = (t0 * b.fs).round() as isize;
    let n = ((dur + tail) * b.fs).round() as usize;
    let gl = ((pan + 1.0) * PI / 4.0).cos();
    let gr = ((pan + 1.0) * PI / 4.0).sin();
    for i in 0..n {
        let ix = i0 + i as isize;
        if ix >= b.left.len() as isize {
            break;
        }
        if ix < 0 {
            continue;
        }
        let t = i as f64 / b.fs;
        let s = sample(t)
            * if t > dur {
                (-(t - dur) * 5.0 / tail).exp()
            } else {
                1.0
            }
            * (t * 400.0).min(1.0);
        let ix = ix as usize;
        b.left[ix] = (b.left[ix] as f64 + s * gl) as f32;
        b.right[ix] = (b.right[ix] as f64 + s * gr) as f32;
    }
}
fn epiano(b: &mut Buffer, t0: f64, note: f64, dur: f64, v: f64, p: f64) {
    let f = hz(note);
    voice(b, t0, dur, 0.5, p, |t| {
        v * (-t * 1.8).exp()
            * (TAU * f * t
                + (1.5 * (-t * 5.0).exp() + 0.3) * (TAU * f * t).sin()
                + 0.5 * (-t * 25.0).exp() * (TAU * f * 14.0 * t).sin())
            .sin()
    });
}
fn bass(b: &mut Buffer, t0: f64, note: f64, dur: f64, v: f64) {
    let f = hz(note);
    voice(b, t0, dur, 0.08, 0.0, |t| {
        v * (-t * 1.2).exp()
            * (TAU * f * t + 0.6 * (-t * 9.0).exp() * (TAU * 2.0 * f * t).sin()).sin()
    });
}
fn kick(b: &mut Buffer, t0: f64, v: f64) {
    voice(b, t0, 0.35, 0.05, 0.0, |t| {
        v * (-t * 8.0).exp() * (TAU * (47.0 * t - 110.0 / 28.0 * ((-t * 28.0).exp() - 1.0))).sin()
    });
}
fn snare(b: &mut Buffer, t0: f64, v: f64, r: &mut Rng) {
    let mut lp = 0.0;
    voice(b, t0, 0.2, 0.05, 0.1, |t| {
        let w = r.uniform() * 2.0 - 1.0;
        lp += 0.35 * (w - lp);
        v * (0.8 * (w - lp) * (-t * 20.0).exp() + 0.6 * (TAU * 185.0 * t).sin() * (-t * 28.0).exp())
    });
}
fn hat(b: &mut Buffer, t0: f64, v: f64, r: &mut Rng, p: f64) {
    let mut z = 0.0;
    voice(b, t0, 0.05, 0.03, p, |t| {
        let w = r.uniform() * 2.0 - 1.0;
        let d = w - z;
        z = w;
        v * d * (-t * 70.0).exp()
    });
}
fn pluck(b: &mut Buffer, t0: f64, note: f64, dur: f64, v: f64, p: f64) {
    let f = hz(note);
    voice(b, t0, dur, 0.4, p, |t| {
        let mut s = 0.0;
        for h in 1..=6 {
            let h = h as f64;
            s += (TAU * f * h * t).sin() * (-t * (2.0 + 2.2 * h)).exp() / h;
        }
        v * s
    });
}
fn pad(b: &mut Buffer, t0: f64, note: f64, dur: f64, v: f64, p: f64) {
    let f = hz(note);
    voice(b, t0, dur, 1.2, p, |t| {
        v * (t / 0.8).min(1.0)
            * ((TAU * f * t).sin()
                + 0.5 * (TAU * f * 1.005 * t + 1.0).sin()
                + 0.3 * (TAU * f * 2.003 * t).sin())
            / 1.8
    });
}
fn finish(mut b: Buffer, title: &str) -> AudioTrack {
    let mut peak = 1e-9_f64;
    for (&l, &r) in b.left.iter().zip(&b.right) {
        peak = peak.max((l as f64).abs()).max((r as f64).abs());
    }
    let g = 0.9 / peak;
    let fade = (0.02 * b.fs).round();
    let n = b.left.len();
    for i in 0..n {
        let w = (i as f64 / fade).min((n - 1 - i) as f64 / fade).min(1.0);
        b.left[i] = (b.left[i] as f64 * g * w) as f32;
        b.right[i] = (b.right[i] as f64 * g * w) as f32;
    }
    AudioTrack {
        left: b.left,
        right: b.right,
        title: title.into(),
    }
}
fn slow_groove(fs: f64) -> AudioTrack {
    let beat = 60.0 / 84.0;
    let bars = 22;
    let mut b = Buffer::new(fs, bars as f64 * 4.0 * beat + 2.0);
    let mut r = Rng::new(3);
    let prog = [
        (50.0, [62.0, 65.0, 69.0, 72.0, 76.0]),
        (43.0, [59.0, 62.0, 65.0, 69.0, 76.0]),
        (48.0, [60.0, 64.0, 67.0, 71.0, 74.0]),
        (45.0, [61.0, 64.0, 67.0, 70.0, 73.0]),
    ];
    for bar in 0..bars {
        let t = bar as f64 * 4.0 * beat;
        let (root, ch) = prog[bar % 4];
        let sw = beat * 0.16;
        for (i, &note) in ch.iter().enumerate() {
            epiano(
                &mut b,
                t + i as f64 * 0.012,
                note,
                beat * 1.8,
                0.16,
                (i as f64 - 2.0) * 0.3,
            );
        }
        for (i, &note) in ch[1..].iter().enumerate() {
            epiano(
                &mut b,
                t + beat * 2.5 + sw + i as f64 * 0.01,
                note,
                beat * 1.2,
                0.11,
                (1.0 - i as f64) * 0.4,
            );
        }
        if bar >= 2 {
            bass(&mut b, t, root - 12.0, beat * 1.4, 0.5);
            bass(&mut b, t + beat * 1.5 + sw, root - 12.0, beat * 0.4, 0.35);
            bass(&mut b, t + beat * 2.5 + sw, root - 5.0, beat * 0.9, 0.4);
            bass(&mut b, t + beat * 3.5 + sw, root - 10.0, beat * 0.4, 0.3);
        }
        if bar >= 4 && bar < bars - 1 {
            kick(&mut b, t, 0.9);
            kick(&mut b, t + beat * 2.5 + sw, 0.75);
            snare(&mut b, t + beat, 0.5, &mut r);
            snare(&mut b, t + beat * 3.0, 0.55, &mut r);
            for h in 0..8 {
                hat(
                    &mut b,
                    t + h as f64 * beat / 2.0 + if h % 2 == 1 { sw } else { 0.0 },
                    if h % 2 == 1 { 0.25 } else { 0.4 },
                    &mut r,
                    0.35,
                );
            }
        }
        if bar >= 8 && bar % 2 == 0 {
            for (i, note) in [81.0, 84.0, 88.0, 86.0].iter().enumerate() {
                pluck(
                    &mut b,
                    t + beat * (1.0 + i as f64 * 0.75),
                    note - if bar % 4 != 0 { 2.0 } else { 0.0 },
                    0.5,
                    0.14,
                    if i % 2 == 1 { 0.6 } else { -0.6 },
                );
            }
        }
    }
    finish(b, "Slow Groove")
}
fn segment(b: &mut Buffer, t0: f64, t1: f64, mut sample: impl FnMut(f64) -> [f64; 2]) {
    for i in (t0 * b.fs).round() as usize..((t1 * b.fs).ceil() as usize).min(b.left.len()) {
        let t = i as f64 / b.fs - t0;
        let w = (t / 0.01).min((t1 - t0 - t) / 0.01).min(1.0);
        let [l, r] = sample(t);
        b.left[i] = (b.left[i] as f64 + l * w) as f32;
        b.right[i] = (b.right[i] as f64 + r * w) as f32;
    }
}
fn test_band(fs: f64) -> AudioTrack {
    let mut b = Buffer::new(fs, 30.0);
    let a = 0.35;
    segment(&mut b, 0.5, 5.0, |t| {
        let s = a * (TAU * 1000.0 * t).sin();
        [s, s]
    });
    segment(&mut b, 5.5, 8.0, |t| [a * (TAU * 1000.0 * t).sin(), 0.0]);
    segment(&mut b, 8.5, 11.0, |t| [0.0, a * (TAU * 1000.0 * t).sin()]);
    segment(&mut b, 11.5, 21.5, |t| {
        let k = 1000.0_f64.ln() / 10.0;
        let s = 0.2 * (TAU * 20.0 * ((k * t).exp() - 1.0) / k).sin();
        [s, s]
    });
    segment(&mut b, 22.0, 25.0, |t| {
        let s = 0.5 * (TAU * 315.0 * t).sin();
        [s, -s]
    });
    AudioTrack {
        left: b.left,
        right: b.right,
        title: "Test Band".into(),
    }
}
fn arps(fs: f64) -> AudioTrack {
    let beat = 60.0 / 112.0;
    let bars = 28;
    let mut b = Buffer::new(fs, bars as f64 * 4.0 * beat + 3.0);
    let mut r = Rng::new(9);
    let prog = [
        (45.0, [57.0, 60.0, 64.0, 67.0, 71.0]),
        (41.0, [57.0, 60.0, 65.0, 69.0, 72.0]),
        (48.0, [55.0, 60.0, 64.0, 67.0, 74.0]),
        (43.0, [55.0, 59.0, 62.0, 67.0, 71.0]),
    ];
    for bar in 0..bars {
        let t = bar as f64 * 4.0 * beat;
        let (root, ch) = prog[bar % 4];
        for (i, &note) in ch[..4].iter().enumerate() {
            pad(&mut b, t, note, beat * 4.0, 0.07, (i as f64 - 1.5) * 0.5);
        }
        for s in 0..16 {
            let note = ch[[0, 2, 4, 1, 3, 4, 2, 1][s % 8]]
                + if s % 8 > 4 { 12.0 } else { 0.0 }
                + if bar >= 12 { 12.0 } else { 0.0 };
            if bar >= 2 {
                pluck(
                    &mut b,
                    t + s as f64 * beat / 4.0,
                    note,
                    0.22,
                    0.2,
                    (s as f64 * 1.3 + bar as f64).sin(),
                );
            }
        }
        if bar >= 6 {
            bass(&mut b, t, root - 12.0, beat * 1.9, 0.5);
            bass(&mut b, t + beat * 2.0, root - 12.0, beat * 0.9, 0.4);
            bass(&mut b, t + beat * 3.0, root, beat * 0.9, 0.35);
        }
        if bar >= 10 && bar < bars - 2 {
            for q in 0..4 {
                kick(&mut b, t + q as f64 * beat, 0.8);
                hat(&mut b, t + (q as f64 + 0.5) * beat, 0.45, &mut r, -0.3);
            }
            snare(&mut b, t + beat, 0.4, &mut r);
            snare(&mut b, t + 3.0 * beat, 0.45, &mut r);
        }
    }
    finish(b, "Arps")
}
pub fn demo_tracks(fs: f64) -> Vec<AudioTrack> {
    vec![slow_groove(fs), test_band(fs), arps(fs)]
}
