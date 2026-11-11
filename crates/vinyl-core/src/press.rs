//! Pressing defects alter wall or disc geometry before playback.
use crate::{
    dsp::{one_pole, Rng, TAU},
    geometry::{groove_radius, theta_for_radius, R_DISC, R_LOCK},
    Record,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Condition {
    pub noise: f64,
    pub dust: f64,
    pub scratches: f64,
    pub warp: f64,
    #[serde(alias = "offCenter")]
    pub off_center: f64,
    pub echo: f64,
}
impl Default for Condition {
    fn default() -> Self {
        Self {
            noise: 1.0,
            dust: 0.25,
            scratches: 0.2,
            warp: 0.3,
            off_center: 0.15,
            echo: 1.0,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Warp {
    pub k: f64,
    pub amp: f64,
    pub ph: f64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Scratch {
    pub r0: f64,
    pub r1: f64,
    pub phi0: f64,
    pub phi1: f64,
}
#[derive(Clone, Debug)]
pub struct Visual {
    pub data: Vec<u8>,
    pub cols: usize,
    pub rows: usize,
}

fn triangular(s: &mut u32) -> f64 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    ((*s & 65535) + (*s >> 16)) as f64 - 65535.0
}
fn bump(a: &mut [f32], i: isize, h: f64, hl: isize) {
    for j in -hl..=hl {
        let k = i + j;
        if k >= 0 && (k as usize) < a.len() {
            a[k as usize] = (a[k as usize] as f64
                + h * 0.5 * (1.0 + (std::f64::consts::PI * j as f64 / hl as f64).cos()))
                as f32;
        }
    }
}

pub fn press(rec: &mut Record, cond: Condition, seed: u32) {
    let mut random = Rng::new(seed);
    let n = rec.n;
    let spr = rec.spr;
    let s = spr.round() as usize;
    let e = 0.006 * cond.echo;
    if e > 0.0 {
        let k = one_pole(2500.0, rec.fs);
        let (mut lp, mut lq) = (0.0, 0.0);
        for i in 0..n.saturating_sub(s) {
            lp += k * (rec.wall_out[i + s] as f64 - lp);
            rec.wall_in[i] = (rec.wall_in[i] as f64 - e * lp) as f32;
            lq += k * (rec.wall_in[i] as f64 - lq);
            rec.wall_out[i + s] = (rec.wall_out[i + s] as f64 - 0.35 * e * lq) as f32;
        }
    }
    if cond.noise > 0.0 {
        let mut state = seed.wrapping_mul(2654435761);
        if state == 0 {
            state = 1;
        }
        let k = 6.0_f64.sqrt() / 65536.0;
        let leak = (-TAU * 12.0 / rec.fs).exp();
        let w = 0.7e-9 * cond.noise * k;
        let r = 1.3e-9 * cond.noise * k;
        let (mut ra, mut rb) = (0.0, 0.0);
        for i in 0..n {
            ra = ra * leak + r * triangular(&mut state);
            rb = rb * leak + r * triangular(&mut state);
            rec.wall_in[i] = (rec.wall_in[i] as f64 + (ra + w * triangular(&mut state))) as f32;
            rec.wall_out[i] = (rec.wall_out[i] as f64 + (rb + w * triangular(&mut state))) as f32;
        }
    }
    let ds_at = |i: usize| TAU * groove_radius(&rec.r_tab, i as f64 * TAU / spr) / spr;
    let n_dust = (cond.dust * 6.0 * n as f64 / spr).round() as usize;
    for _ in 0..n_dust {
        let i = (random.uniform() * n as f64) as usize;
        let size = (5e-6 * (0.7 * random.gauss()).exp()).min(40e-6);
        let hl = (size / ds_at(i)).round().max(2.0) as isize;
        let h = size * (0.2 + 0.3 * random.uniform());
        let w = random.uniform();
        if w < 0.8 {
            bump(
                if w < 0.4 {
                    &mut rec.wall_in
                } else {
                    &mut rec.wall_out
                },
                i as isize,
                h,
                hl,
            );
        } else {
            bump(&mut rec.wall_in, i as isize, h, hl);
            bump(&mut rec.wall_out, i as isize, h, hl);
        }
    }
    let mut scratches = Vec::new();
    for _ in 0..(cond.scratches * 12.0).round() as usize {
        let r0 = 0.062 + random.uniform() * 0.07;
        let len = 0.003 + random.uniform() * 0.022;
        let phi0 = random.uniform() * TAU;
        let slant = (random.uniform() - 0.5) * 0.3;
        let depth = (1.5 + 4.5 * random.uniform()) * 1e-6;
        let sig = (15.0 + 25.0 * random.uniform()) * 1e-6;
        scratches.push(Scratch {
            r0,
            r1: r0 + len,
            phi0,
            phi1: phi0 + slant,
        });
        let mut prev: Option<f64> = None;
        let mut r_prev = r0;
        let mut rr = r0;
        while rr <= r0 + len {
            let phi = phi0 + slant * (rr - r0) / len;
            let f = (theta_for_radius(&rec.r_tab, rr) - phi) / TAU;
            if let Some(fp) = prev {
                if f.floor() != fp.floor() {
                    let m = f.floor().max(fp.floor());
                    let t = (m - fp) / (f - fp);
                    let rc = r_prev + t * (rr - r_prev);
                    let theta = TAU * m + phi0 + slant * (rc - r0) / len;
                    let i = (theta * spr / TAU).round() as isize;
                    if theta > 0.0 && theta < rec.theta_end {
                        let ds = ds_at(i as usize);
                        let hl = (4.0 * sig / ds).ceil() as isize;
                        for j in -hl..=hl {
                            let k = i + j;
                            if k < 0 || k >= n as isize {
                                continue;
                            }
                            let x = j as f64 * ds;
                            let lip = (x.abs() - 2.2 * sig) / (0.6 * sig);
                            let p = depth
                                * (-(-x * x / (2.0 * sig * sig)).exp()
                                    + 0.6 * (-0.5 * lip * lip).exp());
                            rec.wall_in[k as usize] = (rec.wall_in[k as usize] as f64 + p) as f32;
                            rec.wall_out[k as usize] = (rec.wall_out[k as usize] as f64 + p) as f32;
                        }
                    }
                }
            }
            prev = Some(f);
            r_prev = rr;
            rr += 20e-6;
        }
    }
    let ecc = cond.off_center * 1e-3;
    let ea = random.uniform() * TAU;
    rec.ecc = [ecc * ea.cos(), ecc * ea.sin()];
    let wp = cond.warp * 1e-3;
    rec.warp = if wp > 0.0 {
        vec![
            Warp {
                k: 1.0,
                amp: wp * 0.7,
                ph: random.uniform() * TAU,
            },
            Warp {
                k: 2.0,
                amp: wp * 0.35,
                ph: random.uniform() * TAU,
            },
            Warp {
                k: 3.0,
                amp: wp * 0.12,
                ph: random.uniform() * TAU,
            },
        ]
    } else {
        Vec::new()
    };
    rec.scratches = scratches;
    rec.condition = cond;
}
pub fn warp_height(warp: &[Warp], r: f64, phi: f64) -> f64 {
    let t = ((r - R_LOCK) / (R_DISC - R_LOCK)).max(0.0);
    warp.iter()
        .map(|w| w.amp * t * t * (w.k * phi + w.ph).cos())
        .sum()
}
pub fn make_vis(rec: &Record, cols: usize) -> Visual {
    let rows = (rec.theta_end / TAU).ceil() as usize + 1;
    let mut acc = vec![0.0_f32; rows * cols];
    let mut data = vec![0; rows * cols];
    let per = rec.spr / cols as f64;
    for i in 0..rec.n.saturating_sub(1) {
        let d = (rec.wall_in[i + 1] as f64 - rec.wall_in[i] as f64)
            - (rec.wall_out[i + 1] as f64 - rec.wall_out[i] as f64);
        let rev = (i as f64 / rec.spr) as usize;
        let c = ((i as f64 - rev as f64 * rec.spr) / per) as usize;
        if rev * cols + c < acc.len() {
            acc[rev * cols + c] = (acc[rev * cols + c] as f64 + d * d) as f32;
        }
    }
    for rw in 0..rows {
        let ds = TAU * groove_radius(&rec.r_tab, (rw as f64 + 0.5) * TAU) / rec.spr;
        for c in 0..cols {
            let slope = (acc[rw * cols + c] as f64 / per).sqrt() / ds;
            data[rw * cols + c] = (255.0 * (slope / 0.6).sqrt()).round().min(255.0) as u8;
        }
    }
    Visual { data, cols, rows }
}
