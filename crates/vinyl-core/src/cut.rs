//! Mastering lathe: RIAA recording EQ, cutter acceleration/depth preview and variable pitch.
use crate::{
    dsp::{one_pole, riaa_rec_gain, T1, T2, T3, TAU},
    geometry::*,
    Condition, Record, TrackMark,
};
use std::{collections::VecDeque, f64::consts::SQRT_2};
#[derive(Clone, Debug)]
pub struct AudioTrack {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub title: String,
}
#[derive(Clone, Copy, Debug)]
pub struct CutOptions {
    pub cut_peak: f64,
    pub a_max: f64,
    pub elliptical_hz: f64,
    pub z_max: f64,
    pub max_extra: f64,
}
impl Default for CutOptions {
    fn default() -> Self {
        Self {
            cut_peak: 0.14,
            a_max: 12000.0,
            elliptical_hz: 150.0,
            z_max: 9e-6,
            max_extra: 70e-6,
        }
    }
}
fn de_ess(x: &mut [f32], fs: f64, accel_scale: f64, a_max: f64) -> f64 {
    let n = x.len();
    let mut hi = vec![0.0_f32; n];
    let mut g = vec![0.0_f32; n];
    let k = one_pole(3000.0, fs);
    let fs2 = fs * fs;
    let (mut lo, mut h1, mut h2) = (0.0, 0.0, 0.0);
    let mut any = false;
    for i in 0..n {
        lo += k * (x[i] as f64 - lo);
        let h = x[i] as f64 - lo;
        hi[i] = h as f32;
        let a = (h - 2.0 * h1 + h2).abs() * fs2 * accel_scale;
        h2 = h1;
        h1 = h;
        g[i] = if a > a_max { (a_max / a) as f32 } else { 1.0 };
        any |= a > a_max;
    }
    if !any {
        return 0.0;
    }
    let ka = one_pole(1.0 / 0.0008, fs);
    let kr = one_pole(1.0 / 0.03, fs);
    let mut s = 1.0_f64;
    for i in (0..n).rev() {
        s = (g[i] as f64).min(s + (1.0 - s) * ka);
        g[i] = s as f32;
    }
    s = 1.0;
    let mut min_g = 1.0_f64;
    for i in 0..n {
        s = (g[i] as f64).min(s + (1.0 - s) * kr);
        x[i] = (x[i] as f64 - hi[i] as f64 * (1.0 - s)) as f32;
        min_g = min_g.min(s);
    }
    -20.0 * min_g.log10()
}
fn emphasise_and_integrate(x: &mut [f32], fs: f64, scale: f64) {
    if x.is_empty() {
        return;
    }
    let k = 2.0 * fs;
    let b0 = (1.0 + T1 * k) / (1.0 + T2 * k);
    let b1 = (1.0 - T1 * k) / (1.0 + T2 * k);
    let a1 = (1.0 - T2 * k) / (1.0 + T2 * k);
    let leak = (-TAU * 5.0 / fs).exp();
    let kdc = one_pole(5.0, fs);
    let c = 1.0 / (24.0 * fs);
    let (mut xp, mut up) = (0.0, 0.0);
    let (mut um2, mut um1, mut integral, mut lpi) = (0.0, 0.0, 0.0, 0.0);
    let mut shelf = |v: f64| {
        let u = b0 * v + b1 * xp - a1 * up;
        xp = v;
        up = u;
        u
    };
    let mut u0 = shelf(x[0] as f64);
    for i in 0..x.len() {
        let u1 = shelf(if i + 1 < x.len() {
            x[i + 1] as f64
        } else {
            0.0
        });
        integral = leak * integral + (-um2 + 13.0 * um1 + 13.0 * u0 - u1) * c;
        lpi += kdc * (integral - lpi);
        x[i] = (scale * (T3 * u0 + integral - lpi)) as f32;
        um2 = um1;
        um1 = u0;
        u0 = u1;
    }
}
fn depth_control(win: &mut [f32], wout: &mut [f32], fs: f64, z_max: f64) {
    let n = win.len();
    let mut need = vec![0.0_f32; n];
    let mut any = false;
    for i in 0..n {
        let z = (win[i] as f64 + wout[i] as f64) / SQRT_2 - z_max;
        if z > 0.0 {
            need[i] = z as f32;
            any = true;
        }
    }
    if !any {
        return;
    }
    let wm = (0.12 * fs).round() as usize;
    let mut dq: VecDeque<usize> = VecDeque::with_capacity(2 * wm + 2);
    let mut mx = vec![0.0_f32; n];
    for i in 0..n + wm {
        if i < n {
            while dq.back().is_some_and(|&j| need[j] <= need[i]) {
                dq.pop_back();
            }
            dq.push_back(i);
        }
        while dq
            .front()
            .is_some_and(|&j| (j as isize) < i as isize - 2 * wm as isize)
        {
            dq.pop_front();
        }
        if i >= wm {
            mx[i - wm] = dq.front().map(|&j| need[j]).unwrap_or(0.0);
        }
    }
    let h = wm >> 1;
    for pass in 0..2 {
        let (src, dst) = if pass == 0 {
            (&mx, &mut need)
        } else {
            (&need, &mut mx)
        };
        let mut acc = 0.0;
        for i in -(h as isize)..n as isize + h as isize {
            if i + (h as isize) < n as isize {
                acc += src[(i + h as isize) as usize] as f64;
            }
            if i - h as isize - 1 >= 0 {
                acc -= src[(i - h as isize - 1) as usize] as f64;
            }
            if i >= 0 && (i as usize) < n {
                dst[i as usize] = (acc / (2 * h + 1) as f64) as f32;
            }
        }
    }
    for i in 0..n {
        let d = mx[i] as f64 / SQRT_2 * 1.02;
        win[i] = (win[i] as f64 - d) as f32;
        wout[i] = (wout[i] as f64 - d) as f32;
    }
}
/// Cut stereo PCM at `fs` into a variable-pitch 33⅓ rpm record.
/// An empty right channel duplicates the left channel. Long sides truncate at the physical inner limit.
pub fn cut(tracks: &[AudioTrack], fs: f64, o: CutOptions) -> Record {
    assert!(
        fs.is_finite() && fs > 0.0,
        "cut sample rate must be positive"
    );
    let spr = fs * REV_SECONDS;
    let lead_in = (3.0 * spr).round() as usize;
    let gap = (2.0 * spr).round() as usize;
    let tail = spr.round() as usize;
    let mut p = lead_in
        + tail
        + tracks.iter().map(|t| t.left.len()).sum::<usize>()
        + gap * tracks.len().saturating_sub(1);
    let alloc = p + (19.0 * spr).ceil() as usize;
    let mut a = vec![0.0_f32; alloc];
    let mut b = vec![0.0_f32; alloc];
    let mut marks = Vec::new();
    let mut pos = lead_in;
    let mut peak = 1e-3_f64;
    for t in tracks {
        a[pos..pos + t.left.len()].copy_from_slice(&t.left);
        let right = if t.right.is_empty() {
            &t.left
        } else {
            &t.right
        };
        assert_eq!(
            right.len(),
            t.left.len(),
            "stereo channel lengths must match"
        );
        b[pos..pos + right.len()].copy_from_slice(right);
        for (&l, &r) in t.left.iter().zip(right) {
            peak = peak.max((l as f64).abs()).max((r as f64).abs());
        }
        marks.push((t.title.clone(), pos, pos + t.left.len()));
        pos += t.left.len() + gap;
    }
    let vfs = o.cut_peak / peak;
    let scale = vfs / riaa_rec_gain(1000.0);
    let k7 = one_pole(7.0, fs);
    let ke = one_pole(o.elliptical_hz, fs);
    let (mut la, mut lb, mut ls) = (0.0, 0.0, 0.0);
    for i in 0..p {
        la += k7 * (a[i] as f64 - la);
        lb += k7 * (b[i] as f64 - lb);
        let l = a[i] as f64 - la;
        let r = b[i] as f64 - lb;
        let m = (l + r) * 0.5;
        let mut s = (l - r) * 0.5;
        ls += ke * (s - ls);
        s -= ls;
        a[i] = (m + s) as f32;
        b[i] = (m - s) as f32;
    }
    let de_ess_db = de_ess(&mut a[..p], fs, scale * (T1 / T2) * T3, o.a_max).max(de_ess(
        &mut b[..p],
        fs,
        scale * (T1 / T2) * T3,
        o.a_max,
    ));
    emphasise_and_integrate(&mut a[..p], fs, scale);
    emphasise_and_integrate(&mut b[..p], fs, scale);
    for v in &mut b[..p] {
        *v = -*v;
    }
    depth_control(&mut a[..p], &mut b[..p], fs, o.z_max);
    let revs_p = (p as f64 / spr).ceil() as usize;
    let mut e_in = vec![0.0_f64; revs_p + 2];
    let mut e_out = vec![0.0_f64; revs_p + 2];
    for i in 0..p {
        let n = (i as f64 / spr) as usize;
        e_in[n] = e_in[n].max(-a[i] as f64 * SQRT_2);
        e_out[n] = e_out[n].max(-b[i] as f64 * SQRT_2);
    }
    let mut pitch = vec![0.0; revs_p];
    let mut gap_revs = vec![false; revs_p];
    for m in 0..marks.len().saturating_sub(1) {
        let j = ((marks[m].2 as f64 + gap as f64 * 0.25) / spr).floor() as usize;
        if j < revs_p {
            gap_revs[j] = true;
        }
    }
    let mut need = 0.0;
    for n in 0..revs_p {
        pitch[n] =
            2.0 * GROOVE_W + LAND_MIN + e_in[n].max(e_in[n + 1]) + e_out[n].max(e_out[n + 1]);
        if n < 2 {
            pitch[n] = (R_LEADIN - R_PROGRAM) / 2.0;
        } else {
            if gap_revs[n] {
                pitch[n] += 0.6e-3;
            }
            need += pitch[n];
        }
    }
    let room = R_PROGRAM - R_PROGRAM_MIN;
    let extra = ((room - need) / revs_p.saturating_sub(2).max(1) as f64)
        .min(o.max_extra)
        .max(0.0);
    let mut revs_used = revs_p;
    let mut acc = 0.0;
    let mut truncated = false;
    for n in 2..revs_p {
        pitch[n] += extra;
        acc += pitch[n];
        if acc > room {
            revs_used = n;
            truncated = true;
            break;
        }
    }
    if truncated {
        p = (revs_used as f64 * spr).floor() as usize;
    }
    let r_end_prog = R_LEADIN - pitch[..revs_used].iter().sum::<f64>();
    let lo_revs = ((r_end_prog - R_LOCK) / 6.4e-3).ceil().max(2.0) as usize;
    let prog_knots = (p as f64 / spr * TAB_PER_REV as f64).ceil() as usize;
    let lo_knots = lo_revs * TAB_PER_REV;
    let lock_knots = (1.5 * TAB_PER_REV as f64).round() as usize;
    let nk = prog_knots + lo_knots + lock_knots + 1;
    let mut pk = vec![0.0; nk];
    let mut tmp = vec![0.0; nk];
    let mut r_tab = vec![0.0; nk];
    let mut lo_pitch = (r_end_prog - R_LOCK) / lo_revs as f64;
    for _ in 0..3 {
        for k in 0..nk {
            pk[k] = if k < prog_knots {
                pitch[(k / TAB_PER_REV).min(revs_used - 1)]
            } else if k < prog_knots + lo_knots {
                lo_pitch
            } else {
                0.0
            };
        }
        for pass in 0..2 {
            let (s, d) = if pass == 0 {
                (&pk, &mut tmp)
            } else {
                (&tmp, &mut pk)
            };
            for k in 0..nk {
                let mut sum = 0.0;
                for j in -8..=8 {
                    let q = k as isize + j;
                    sum += if q < 0 {
                        s[0]
                    } else if q >= nk as isize {
                        0.0
                    } else {
                        s[q as usize]
                    };
                }
                d[k] = sum / 17.0;
            }
        }
        let mut r = R_LEADIN;
        for k in 0..nk {
            r_tab[k] = r;
            r -= pk[k] / TAB_PER_REV as f64;
        }
        lo_pitch += (r_tab[nk - 1] - R_LOCK) / lo_revs as f64;
    }
    let theta_end = (nk - 1) as f64 * TAU / TAB_PER_REV as f64;
    let n = alloc.min((theta_end * spr / TAU).ceil() as usize);
    if truncated {
        a[p..n].fill(0.0);
        b[p..n].fill(0.0);
    }
    a.truncate(n);
    b.truncate(n);
    let tracks = marks
        .into_iter()
        .filter(|m| m.1 < p)
        .map(|(title, i0, i1)| TrackMark {
            title,
            theta0: i0 as f64 * TAU / spr,
            theta1: i1.min(p) as f64 * TAU / spr,
        })
        .collect();
    Record {
        fs,
        spr,
        n,
        wall_in: a,
        wall_out: b,
        r_tab,
        theta_end,
        theta_lock: theta_end - TAU,
        w: GROOVE_W,
        vfs,
        de_ess_db,
        truncated,
        tracks,
        ecc: [0.0; 2],
        warp: Vec::new(),
        scratches: Vec::new(),
        condition: Condition::default(),
    }
}
