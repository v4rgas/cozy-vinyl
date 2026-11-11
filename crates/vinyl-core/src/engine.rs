//! Coupled platter, arm, two-degree-of-freedom stylus, unilateral Hertz contacts,
//! Zener cantilever, moving-magnet generator, electrical load and RIAA playback.
use crate::{dsp::*, geometry::*, groove::GrooveReader, press::warp_height, Record};
use serde::{Deserialize, Serialize};
use std::{
    f64::consts::{FRAC_1_SQRT_2 as IS2, SQRT_2 as SQ2},
    sync::Arc,
};
const DT: usize = 12;
const PH: usize = 64;
const WLEN: usize = 128;
const G: f64 = 9.80665;
const E_VINYL: f64 = 3.4e9;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Stylus {
    Conical,
    #[default]
    Elliptical,
    Microline,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Parameters {
    pub vtf: f64,
    pub antiskate: f64,
    pub m_eff: f64,
    pub kc: f64,
    pub k1: f64,
    pub c1: f64,
    pub c0: f64,
    pub mu: f64,
    pub hc: f64,
    pub azimuth: f64,
    pub lc: f64,
    pub rc: f64,
    pub cl: f64,
    pub rl: f64,
    pub volume: f64,
    pub hum: f64,
    pub rumble: f64,
    pub flutter: f64,
    pub pre_noise: f64,
    pub mt: f64,
    pub r: f64,
    pub k_h: f64,
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            vtf: 1.8e-3 * G,
            antiskate: 0.115 * 1.8e-3 * G,
            m_eff: 0.0185,
            kc: 50.0,
            k1: 420.0,
            c1: 0.2,
            c0: 0.004,
            mu: 0.3,
            hc: 2.0,
            azimuth: 1.2 * std::f64::consts::PI / 180.0,
            lc: 0.45,
            rc: 600.0,
            cl: 200e-12,
            rl: 47000.0,
            volume: 1.0,
            hum: 0.0,
            rumble: 1.0,
            flutter: 1.0,
            pre_noise: 1e-5,
            mt: 0.35e-6,
            r: 18e-6,
            k_h: (4.0 / 3.0) * E_VINYL * (7.6e-6_f64 * 18e-6).sqrt().sqrt(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Telemetry {
    pub thp: f64,
    pub wp: f64,
    pub al: f64,
    pub zb: f64,
    pub ze: f64,
    pub xi: f64,
    pub cue_up: bool,
    pub motor: bool,
    pub hand: bool,
    pub auto: u8,
    pub theta: f64,
    pub r: f64,
    pub err: f64,
    pub vg: f64,
    pub hc: f64,
    pub xg: f64,
    pub n_in: f64,
    pub n_out: f64,
    pub loss: f64,
    pub peak: f64,
    pub blocks: u64,
    pub blown: u64,
}
#[derive(Default)]
struct Accum {
    n_in: f64,
    n_out: f64,
    loss: f64,
    cnt: usize,
    peak: f64,
    theta: f64,
    r: f64,
    err: f64,
    vg: f64,
    hc: f64,
    xg: f64,
}
pub struct Engine {
    pub fs: f64,
    pub oversampling: usize,
    pub fs_fast: f64,
    pub p: Parameters,
    pub rec: Option<Arc<Record>>,
    pub stylus: Stylus,
    pub thp: f64,
    pub wp: f64,
    pub motor: bool,
    pub w_set: f64,
    pub t: f64,
    pub drift: f64,
    pub hp_on: bool,
    pub hp_t: f64,
    pub drag_tq: f64,
    pub al: f64,
    pub ald: f64,
    pub zb: f64,
    pub zbd: f64,
    pub cue_up: bool,
    pub zcue: f64,
    pub h_on: bool,
    pub h_al: f64,
    pub auto: u8,
    pub auto_al: f64,
    pub xi: f64,
    pub xid: f64,
    pub ze: f64,
    pub zed: f64,
    pub mwx: f64,
    pub mwz: f64,
    pub gn: f64,
    pub xg_p: f64,
    pub hc_p: f64,
    pub dp_in: f64,
    pub dp_out: f64,
    pub zs: f64,
    pub kick: f64,
    pub bs: f64,
    pub vtf_s: f64,
    pub blown: u64,
    pub blown_why: [f64; 7],
    reader: GrooveReader,
    c: Vec<f32>,
    nt: usize,
    h: Vec<f32>,
    ring_l: Vec<f32>,
    ring_r: Vec<f32>,
    rp: usize,
    w_in: [f32; WLEN],
    w_out: [f32; WLEN],
    wj0: isize,
    random: Rng,
    lcr: [f64; 5],
    riaa_f: [f64; 5],
    riaa_s: [f64; 5],
    fz: [f64; 12],
    ru: [f64; 12],
    tel: Accum,
}
impl Engine {
    pub fn new(fs: f64) -> Self {
        assert!(
            fs.is_finite() && fs > 0.0,
            "playback sample rate must be positive"
        );
        let os = (340000.0 / fs).ceil().clamp(2.0, 8.0) as usize;
        let ff = fs * os as f64;
        let nt = 32 * os;
        let p = Parameters::default();
        let mut e = Self {
            fs,
            oversampling: os,
            fs_fast: ff,
            p,
            rec: None,
            stylus: Stylus::Elliptical,
            thp: 0.0,
            wp: 0.0,
            motor: false,
            w_set: W33,
            t: 0.0,
            drift: 0.0,
            hp_on: false,
            hp_t: 0.0,
            drag_tq: 0.0,
            al: alpha_rest(),
            ald: 0.0,
            zb: 5e-3,
            zbd: 0.0,
            cue_up: true,
            zcue: 5e-3,
            h_on: false,
            h_al: alpha_rest(),
            auto: 0,
            auto_al: 0.0,
            xi: 0.0,
            xid: 0.0,
            ze: 0.0,
            zed: 0.0,
            mwx: 0.0,
            mwz: 0.0,
            gn: 0.0,
            xg_p: 0.0,
            hc_p: 1.0,
            dp_in: -1.0,
            dp_out: -1.0,
            zs: 0.0,
            kick: 0.0,
            bs: 0.0,
            vtf_s: p.vtf,
            blown: 0,
            blown_why: [0.0; 7],
            reader: GrooveReader::new(),
            c: make_polyphase(DT, PH, 1.0, 7.5),
            nt,
            h: make_lowpass(nt, 0.96 / os as f64, 7.0),
            ring_l: vec![0.0; 2 * nt],
            ring_r: vec![0.0; 2 * nt],
            rp: 0,
            w_in: [0.0; WLEN],
            w_out: [0.0; WLEN],
            wj0: -1_000_000_000,
            random: Rng::new(12345),
            lcr: [0.0; 5],
            riaa_f: riaa_playback(ff),
            riaa_s: riaa_playback(fs),
            fz: [0.0; 12],
            ru: [0.0; 12],
            tel: Accum {
                hc: 1.0,
                ..Accum::default()
            },
        };
        e.set_stylus(Stylus::Elliptical);
        e
    }
    pub fn set_stylus(&mut self, stylus: Stylus) {
        let (scan, bear, seat, mt): (f64, f64, f64, f64) = match stylus {
            Stylus::Conical => (18e-6, 18e-6, 18e-6, 0.6e-6),
            Stylus::Elliptical => (7.6e-6, 18e-6, 18e-6, 0.35e-6),
            Stylus::Microline => (2.5e-6, 75e-6, 18e-6, 0.25e-6),
        };
        self.stylus = stylus;
        self.p.mt = mt;
        self.p.r = seat;
        self.p.k_h = (4.0 / 3.0) * E_VINYL * (scan * bear).sqrt().sqrt();
        self.reader.set_scan(scan);
        self.wj0 = -1_000_000_000;
        self.update_electrical();
    }
    pub fn set_vtf(&mut self, grams: f64) {
        self.p.vtf = grams * 1e-3 * G;
    }
    pub fn set_antiskate(&mut self, grams: f64) {
        self.p.antiskate = 0.115 * grams * 1e-3 * G;
    }
    pub fn set_capacitance(&mut self, pf: f64) {
        self.p.cl = pf * 1e-12;
        self.update_electrical();
    }
    pub fn apply_parameters(&mut self, p: Parameters) {
        self.p = p;
        self.update_electrical();
    }
    fn update_electrical(&mut self) {
        let p = self.p;
        let k = 1.0 + p.rc / p.rl;
        self.lcr = bilinear(
            [1.0, 0.0, 0.0],
            [1.0, (p.lc / p.rl + p.rc * p.cl) / k, p.lc * p.cl / k],
            self.fs_fast,
        );
    }
    /// Installs an immutable master; return the retired master for disposal off the audio thread.
    pub fn set_record(&mut self, record: impl Into<Arc<Record>>) -> Option<Arc<Record>> {
        self.reader.reset();
        self.wj0 = -1_000_000_000;
        self.gn = 0.0;
        self.rec.replace(record.into())
    }
    pub fn set_motor(&mut self, on: bool) {
        self.motor = on;
    }
    pub fn set_speed(&mut self, rpm: f64) {
        self.w_set = TAU * rpm / 60.0;
    }
    pub fn set_cue(&mut self, up: bool) {
        self.cue_up = up;
        self.auto = 0;
        self.h_on = false;
    }
    pub fn hand(&mut self, on: bool, al: f64) {
        self.auto = 0;
        if on {
            self.h_on = true;
            self.cue_up = true;
            self.h_al = al.clamp(alpha_min(), alpha_rest());
        } else {
            self.h_on = false;
            self.cue_up = false;
        }
    }
    pub fn cue_to(&mut self, al: f64) {
        self.auto = 1;
        self.auto_al = al.clamp(alpha_min(), alpha_rest());
        self.cue_up = true;
        self.h_on = true;
        self.h_al = self.al;
    }
    pub fn platter_hand(&mut self, on: bool) {
        self.hp_on = on;
        self.hp_t = self.thp;
    }
    pub fn platter_move(&mut self, dtheta: f64) {
        self.hp_t += dtheta;
    }
    pub fn knock(&mut self, strength: f64) {
        self.kick += strength;
    }
    pub fn telemetry(&mut self) -> Telemetry {
        let t = &mut self.tel;
        let c = t.cnt.max(1) as f64;
        let out = Telemetry {
            thp: self.thp,
            wp: self.wp,
            al: self.al,
            zb: self.zb,
            ze: self.ze,
            xi: self.xi,
            cue_up: self.cue_up,
            motor: self.motor,
            hand: self.h_on,
            auto: self.auto,
            theta: t.theta,
            r: t.r,
            err: t.err,
            vg: t.vg,
            hc: t.hc,
            xg: t.xg,
            n_in: t.n_in / c,
            n_out: t.n_out / c,
            loss: t.loss / c,
            peak: t.peak,
            blocks: self.reader.computed,
            blown: self.blown,
        };
        t.n_in = 0.0;
        t.n_out = 0.0;
        t.loss = 0.0;
        t.cnt = 0;
        t.peak = 0.0;
        out
    }
    /// Raw wall top geometry in metres, for the microscope (last pair = current tip x and height).
    pub fn groove_view(&self, rows: usize, cols: usize, span: f64) -> Vec<f32> {
        let mut out = vec![0.0; rows * cols * 2 + 2];
        let Some(rec) = self.rec.as_ref() else {
            return out;
        };
        let th = self.tel.theta;
        let s0 = th * rec.spr / TAU;
        let rg0 = groove_radius(&rec.r_tab, th);
        let half = (rows as f64 - 1.0) / 2.0;
        for g in 0..rows {
            for c in 0..cols {
                let ds = (c as f64 / (cols.saturating_sub(1).max(1)) as f64 * 2.0 - 1.0) * span;
                let thg = th + (g as f64 - half) * TAU + ds * TAU / rec.spr;
                let i = nearest_integer(s0 + (g as f64 - half) * rec.spr + ds) as isize;
                let o = (g * cols + c) * 2;
                if thg < 0.0 || thg > rec.theta_end || i < 0 || i >= rec.n as isize {
                    out[o] = f32::NAN;
                    out[o + 1] = f32::NAN;
                    continue;
                }
                let rc = groove_radius(&rec.r_tab, thg) - rg0;
                out[o] = (rc - rec.w + SQ2 * rec.wall_in[i as usize] as f64) as f32;
                out[o + 1] = (rc + rec.w - SQ2 * rec.wall_out[i as usize] as f64) as f32;
            }
        }
        out[rows * cols * 2] = self.tel.xg as f32;
        out[rows * cols * 2 + 1] = self.tel.hc as f32;
        out
    }
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len().min(out_r.len());
        if n == 0 {
            return;
        }
        let p = self.p;
        let rec = self.rec.as_deref();
        let os = self.oversampling;
        let fs_f = self.fs_fast;
        let dt = 1.0 / fs_f;
        let dt_s = 1.0 / self.fs;
        let theta_end = rec.map_or(0.0, |r| r.theta_end);
        let kd = rec.map_or(0.0, |r| 2.0 * r.spr / TAU);
        let (ex, ey) = rec.map_or((0.0, 0.0), |r| (r.ecc[0], r.ecc[1]));
        let warp = rec.map_or(&[][..], |r| r.warp.as_slice());
        let width = rec.map_or(30e-6, |r| r.w);
        let radius = p.r;
        let (kc, k1, c0, mu, kh, mt, m_eff, vtf) =
            (p.kc, p.k1, p.c0, p.mu, p.k_h, p.mt, p.m_eff, p.vtf);
        let ch = 1.5 * p.hc;
        let dc = (mt / (dt * dt) / (1.5 * kh)).powi(2);
        let nc = kh * dc * dc.sqrt();
        let k_contact = 1.5 * kh * dc.sqrt();
        let kmw = 1.0 - (-dt * k1 / p.c1).exp();
        let ih = m_eff * ARM_LE * ARM_LE;
        let kq = m_eff * 9e4;
        let cq = 2.0 * m_eff * 300.0;
        let tq_as = p.antiskate * ARM_LE;
        let caz = p.azimuth.cos();
        let saz = p.azimuth.sin();
        let cb = arm_beta().cos();
        let sb = arm_beta().sin();
        let px = pivot_x();
        let py = pivot_y();
        let [lb0, lb1, lb2, la1, la2] = self.lcr;
        let [rb0, rb1, rb2, ra1, ra2] = self.riaa_f;
        let [sb0, sb1, sb2, sa1, sa2] = self.riaa_s;
        let kv = one_pole(1.5, self.fs);
        let kr1 = one_pole(30.0, self.fs);
        let kr2 = one_pole(6.0, self.fs);
        let kdr = one_pole(1.5, self.fs);
        let rum_s = p.rumble * 0.1e-6 / (0.289 * (kr1 * 3.0 / 16.0).sqrt());
        let gain = p.volume / rec.map_or(1.0, |r| r.vfs);
        let [mut l1, mut l2, mut l3, mut l4, mut r1, mut r2, mut r3, mut r4, mut s1, mut s2, mut s3, mut s4] =
            self.fz;
        let (
            mut thp,
            mut wp,
            mut al,
            mut ald,
            mut zb,
            mut zbd,
            mut xi,
            mut xid,
            mut ze,
            mut zed,
            mut mwx,
            mut mwz,
            mut gn,
            mut xg_p,
            mut hc_p,
            mut dp_in,
            mut dp_out,
            mut zs,
            mut zcue,
            mut rp,
            mut wj0,
            mut t,
            mut drift,
            mut drag_tq,
            mut bs,
            mut vtf_s,
        ) = (
            self.thp,
            self.wp,
            self.al,
            self.ald,
            self.zb,
            self.zbd,
            self.xi,
            self.xid,
            self.ze,
            self.zed,
            self.mwx,
            self.mwz,
            self.gn,
            self.xg_p,
            self.hc_p,
            self.dp_in,
            self.dp_out,
            self.zs,
            self.zcue,
            self.rp,
            self.wj0,
            self.t,
            self.drift,
            self.drag_tq,
            self.bs,
            self.vtf_s,
        );
        let motor = self.motor;
        let hp_on = self.hp_on;
        let w_set = self.w_set;
        let flut = p.flutter;
        let ru = &mut self.ru;
        let tel = &mut self.tel;
        for i in 0..n {
            t += dt_s;
            let h_on = self.h_on;
            let h_al = self.h_al;
            // Motor servo + compliant belt, bearing friction and previous-sample stylus drag.
            drift += kdr * ((self.random.uniform() - 0.5) * 0.07 - drift);
            let wref = w_set * (1.0 + flut * (2.5e-4 * (TAU * 4.6 * t).sin() + drift));
            let mut tq = -0.004 * wp;
            if motor {
                bs += (wref - wp) * dt_s;
                tq = 10.7 * bs + 0.4 * (wref - wp);
                if tq > 0.045 {
                    tq = 0.045;
                    bs = bs.min(0.045 / 10.7);
                } else if tq < -0.045 {
                    tq = -0.045;
                    bs = bs.max(-0.045 / 10.7);
                }
            } else {
                bs = 0.0;
            }
            tq -= 2e-4 * wp + 3e-4 * wp / (wp.abs() + 0.02) + drag_tq;
            if hp_on {
                tq += 0.017 * 120.0 * 120.0 * (self.hp_t - thp) - 2.0 * 0.017 * 120.0 * wp;
            }
            wp += tq / 0.017 * dt_s;
            thp += wp * dt_s;
            zcue +=
                ((if self.cue_up { 5e-3 } else { -2e-3 }) - zcue).clamp(-0.006 * dt_s, 0.03 * dt_s);
            if self.auto == 1 {
                if zb > 5e-3 - 5e-4 {
                    self.auto = 2;
                }
            } else if self.auto == 2 {
                self.h_al += (self.auto_al - self.h_al).clamp(-0.8 * dt_s, 0.8 * dt_s);
                if (self.auto_al - al).abs() < 2e-4 && ald.abs() < 2e-3 {
                    self.auto = 0;
                    self.h_on = false;
                    self.cue_up = self.auto_al >= alpha_rest() - 0.02;
                }
            }
            // Bearing displacement: 6–30 Hz; plinth on spring feet: 12 Hz damped oscillator.
            ru[0] += kr1 * (self.random.uniform() - 0.5 - ru[0]);
            ru[1] += kr1 * (ru[0] - ru[1]);
            ru[2] += kr1 * (ru[1] - ru[2]);
            ru[3] += kr2 * (ru[2] - ru[3]);
            ru[4] += kr1 * (self.random.uniform() - 0.5 - ru[4]);
            ru[5] += kr1 * (ru[4] - ru[5]);
            ru[6] += kr1 * (ru[5] - ru[6]);
            ru[7] += kr2 * (ru[6] - ru[7]);
            if self.kick != 0.0 {
                ru[9] += 0.012 * self.kick;
                self.kick = 0.0;
            }
            if ru[9] != 0.0 {
                ru[9] += (-5685.0 * ru[8] - 45.0 * ru[9]) * dt_s;
                ru[8] += ru[9] * dt_s;
                if ru[8].abs() < 1e-12 && ru[9].abs() < 1e-10 {
                    ru[8] = 0.0;
                    ru[9] = 0.0;
                }
            }
            let ca = al.cos();
            let sa = al.sin();
            let cp = thp.cos();
            let sp = thp.sin();
            let fx = ca * cb + sa * sb;
            let fy = sa * cb - ca * sb;
            let lx = -fy;
            let ly = fx;
            let cx = ex * cp + ey * sp;
            let cy = -ex * sp + ey * cp;
            let qx = px + ARM_LE * ca - cx + 0.5 * rum_s * (ru[6] - ru[7]) + 0.5 * ru[8];
            let qy = py + ARM_LE * sa - cy;
            let r = qx.hypot(qy);
            let erx = qx / r;
            let ery = qy / r;
            let th0 = qy.atan2(qx) + thp;
            let cdx = wp * cy;
            let cdy = -wp * cx;
            let c_sr = ARM_LE * (-sa * erx + ca * ery);
            let c_sph = ARM_LE * (sa * ery + ca * erx) / r;
            let ecc_r = -(cdx * erx + cdy * ery);
            let ecc_ph = (cdx * ery - cdy * erx) / r;
            let c_lr = lx * erx + ly * ery;
            let c_lph = (-lx * ery + ly * erx) / r;
            let c_lt = -c_lph * r;
            let c_fr = fx * erx + fy * ery;
            let c_ft = fx * ery - fy * erx;
            let ramp = -2e-3 + (5e-3 + 2e-3) * ((r - (R_DISC - 5e-4)) / 0.003).clamp(0.0, 1.0);
            let sup = zcue.max(ramp);
            vtf_s += kv * (vtf - vtf_s);
            let zs_new = warp_height(warp, r, th0) + rum_s * (ru[2] - ru[3]) + ru[8];
            let dzs = (zs_new - zs) / os as f64;
            let r_t = r + xi * c_lr;
            let (mut flat, mut rg, mut rg1, mut th) = (true, 0.0, 0.0, 0.0);
            if let Some(record) = rec {
                let tab = &record.r_tab;
                if hc_p > radius + 30e-6 {
                    gn = nearest_integer((theta_for_radius(tab, r_t) - th0) / TAU);
                }
                th = th0 + TAU * gn;
                if th >= theta_end {
                    let k = ((th - theta_end) / TAU).floor() + 1.0;
                    gn -= k;
                    th -= k * TAU;
                }
                if th >= 0.0 {
                    rg = groove_radius(tab, th);
                    let xg0 = r_t - rg;
                    if xg0 > 20e-6 {
                        if th - TAU >= 0.0 && xg0 > 0.5 * (groove_radius(tab, th - TAU) - rg) {
                            gn -= 1.0;
                            th -= TAU;
                            rg = groove_radius(tab, th);
                        }
                    } else if xg0 < -20e-6
                        && th + TAU < theta_end
                        && -xg0 > 0.5 * (rg - groove_radius(tab, th + TAU))
                    {
                        gn += 1.0;
                        th += TAU;
                        rg = groove_radius(tab, th);
                    }
                    rg1 = (groove_radius(tab, th + 0.02) - rg) * 50.0;
                    flat = false;
                }
            }
            let vg = r * (wp + c_sph * ald + ecc_ph);
            let mut th_b = th;
            let mut r_b = r;
            let (mut n_in, mut n_out, mut dg_sum, mut hc, mut xg) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for _ in 0..os {
                th_b += (wp + c_sph * ald + ecc_ph) * dt;
                r_b += (c_sr * ald + ecc_r) * dt;
                zs += dzs;
                let th_t = th_b + xi * c_lph;
                xg = r_b + xi * c_lr - (rg + rg1 * (th_t - th));
                hc = zb + ze - zs;
                let (mut fr, mut fz, mut dg) = (0.0, 0.0, 0.0);
                n_in = 0.0;
                n_out = 0.0;
                if hc < radius + 2e-6 {
                    let (mut din, mut dout) = (0.0, 0.0);
                    if !flat && xg < 80e-6 && xg > -80e-6 {
                        let pos = th_t * kd;
                        let j = pos.floor() as isize;
                        if j - 5 < wj0 || j + 6 >= wj0 + WLEN as isize {
                            wj0 = j - 32;
                            if let Some(record) = rec {
                                self.reader.fill(
                                    record,
                                    wj0,
                                    WLEN,
                                    &mut self.w_in,
                                    &mut self.w_out,
                                );
                            }
                        }
                        let f = (pos - j as f64) * PH as f64;
                        let ph = f as usize;
                        let g = f - ph as f64;
                        let o = ph * DT;
                        let ib = (j - 5 - wj0) as usize;
                        for q in 0..DT {
                            let mut c = self.c[o + q] as f64;
                            c += g * (self.c[o + DT + q] as f64 - c);
                            din += c * self.w_in[ib + q] as f64;
                            dout += c * self.w_out[ib + q] as f64;
                        }
                    }
                    let vx = (xg - xg_p) * fs_f;
                    let vz = (hc - hc_p) * fs_f;
                    let (mut d1, mut nx, mut nz) = if flat {
                        (radius - hc, 0.0, 1.0)
                    } else {
                        let (gap, nx, nz) = gap_to(xg, hc, din, width, radius);
                        (-gap, nx, nz)
                    };
                    if d1 > 0.0 {
                        let mut nn = if d1 < dc {
                            kh * d1 * d1.sqrt()
                        } else {
                            nc + k_contact * (d1 - dc)
                        };
                        if dp_in > 0.0 {
                            nn *= (1.0 + ch * (d1 - dp_in) * fs_f).clamp(0.0, 3.0);
                        }
                        let vt = vx * nz - vz * nx;
                        let vm = (vg * vg + vt * vt + 1e-6).sqrt();
                        let ft = -mu * nn * vt / vm;
                        fr += nn * nx + ft * nz;
                        fz += nn * nz - ft * nx;
                        dg += mu * nn * vg / vm;
                        n_in = nn;
                    }
                    dp_in = d1;
                    if !flat {
                        let (gap, gx, gz) = gap_to(-xg, hc, dout, width, radius);
                        d1 = -gap;
                        nx = -gx;
                        nz = gz;
                        if d1 > 0.0 {
                            let mut nn = if d1 < dc {
                                kh * d1 * d1.sqrt()
                            } else {
                                nc + k_contact * (d1 - dc)
                            };
                            if dp_out > 0.0 {
                                nn *= (1.0 + ch * (d1 - dp_out) * fs_f).clamp(0.0, 3.0);
                            }
                            let vt = vx * nz - vz * nx;
                            let vm = (vg * vg + vt * vt + 1e-6).sqrt();
                            let ft = -mu * nn * vt / vm;
                            fr += nn * nx + ft * nz;
                            fz += nn * nz - ft * nx;
                            dg += mu * nn * vg / vm;
                            n_out = nn;
                        }
                        dp_out = d1;
                    }
                } else {
                    dp_in = -1.0;
                    dp_out = -1.0;
                }
                xg_p = xg;
                hc_p = hc;
                dg_sum += dg;
                // Zener suspension memory, tonearm horizontal/vertical motion, tip inertial motion.
                let fsx = kc * xi + k1 * (xi - mwx) + c0 * xid;
                let fsz = kc * ze + k1 * (ze - mwz) + c0 * zed;
                let mut fq = 0.0;
                if zb < sup {
                    fq = (kq * (sup - zb) - cq * zbd).max(0.0);
                }
                let mut ta = ARM_LE * (-(fr * c_fr + dg * c_ft) * sb + fsx * cb) - 1e-5 * ald;
                if fq > 0.0 {
                    ta -= ih * 150.0 * ald;
                } else {
                    ta += tq_as;
                }
                if h_on {
                    ta += ih * (40.0 * 40.0 * (h_al - al) - 2.0 * 40.0 * ald);
                }
                let aldd = ta / ih;
                let zbdd = (fsz - vtf_s + fq - 0.03 * zbd) / m_eff;
                xid += ((fr * c_lr + dg * c_lt - fsx) / mt - ARM_LE * aldd * cb) * dt + 1e-22;
                xi += xid * dt;
                zed += ((fz - fsz) / mt - zbdd) * dt + 1e-22;
                ze += zed * dt;
                ald += aldd * dt + 1e-22;
                al += ald * dt;
                zbd += zbdd * dt + 1e-22;
                zb += zbd * dt;
                mwx += (xi - mwx) * kmw;
                mwz += (ze - mwz) * kmw;
                let vxr = xid * caz - zed * saz;
                let vzr = xid * saz + zed * caz;
                let el = (vxr + vzr) * IS2 + 1e-30;
                let er = (vxr - vzr) * IS2 + 1e-30;
                let mut y = lb0 * el + l1;
                l1 = lb1 * el - la1 * y + l2;
                l2 = lb2 * el - la2 * y;
                let mut u = rb0 * y + r1;
                r1 = rb1 * y - ra1 * u + r2;
                r2 = rb2 * y - ra2 * u;
                self.ring_l[rp] = u as f32;
                self.ring_l[rp + self.nt] = u as f32;
                y = lb0 * er + l3;
                l3 = lb1 * er - la1 * y + l4;
                l4 = lb2 * er - la2 * y;
                u = rb0 * y + r3;
                r3 = rb1 * y - ra1 * u + r4;
                r4 = rb2 * y - ra2 * u;
                self.ring_r[rp] = u as f32;
                self.ring_r[rp + self.nt] = u as f32;
                rp += 1;
                if rp == self.nt {
                    rp = 0;
                }
            }
            if al > alpha_rest() {
                al = alpha_rest();
                if ald > 0.0 {
                    ald = 0.0;
                }
            }
            if al < alpha_min() {
                al = alpha_min();
                if ald < 0.0 {
                    ald = 0.0;
                }
            }
            if zb > 0.012 {
                zb = 0.012;
                if zbd > 0.0 {
                    zbd = 0.0;
                }
            } else if zb < -4e-3 {
                zb = -4e-3;
                if zbd < 0.0 {
                    zbd = 0.0;
                }
            }
            drag_tq = dg_sum / os as f64 * r;
            let (mut a_l, mut a_r) = (0.0, 0.0);
            for q in 0..self.nt {
                a_l += self.h[q] as f64 * self.ring_l[rp + q] as f64;
                a_r += self.h[q] as f64 * self.ring_r[rp + q] as f64;
            }
            let hm = if p.hum != 0.0 {
                p.hum * (3e-5 * (TAU * 60.0 * t).sin() + 1.2e-5 * (TAU * 180.0 * t + 1.0).sin())
            } else {
                0.0
            };
            let nl = hm + p.pre_noise * 3.46 * (self.random.uniform() - 0.5);
            let nr = 0.8 * hm + p.pre_noise * 3.46 * (self.random.uniform() - 0.5);
            let yl = sb0 * nl + s1;
            s1 = sb1 * nl - sa1 * yl + s2;
            s2 = sb2 * nl - sa2 * yl;
            let yr = sb0 * nr + s3;
            s3 = sb1 * nr - sa1 * yr + s4;
            s4 = sb2 * nr - sa2 * yr;
            let ol = (a_l + yl) * gain;
            let or = (a_r + yr) * gain;
            out_l[i] = ol as f32;
            out_r[i] = or as f32;
            tel.n_in += n_in;
            tel.n_out += n_out;
            tel.cnt += 1;
            if !flat && hc < radius && (n_in == 0.0 || n_out == 0.0) {
                tel.loss += 1.0;
            }
            let pk = ol.abs().max(or.abs());
            if pk > tel.peak {
                tel.peak = pk;
            }
            if i == n - 1 {
                tel.theta = th;
                tel.r = r_t;
                tel.err = c_lt;
                tel.vg = vg;
                tel.hc = hc;
                tel.xg = if flat { f64::NAN } else { xg };
            }
        }
        // Same containment guard as the browser model: recover from pathological tracking states.
        if !(xi.abs() < 5e-3)
            || !(ze.abs() < 5e-3)
            || !(zb.abs() < 0.05)
            || !al.is_finite()
            || !(wp.abs() < 60.0)
        {
            self.blown_why = [xi, ze, zb, al, wp, xid, zed];
            xi = 0.0;
            xid = 0.0;
            ze = 0.0;
            zed = 0.0;
            mwx = 0.0;
            mwz = 0.0;
            ald = 0.0;
            zbd = 0.0;
            zb = 5e-3;
            zcue = 5e-3;
            self.cue_up = true;
            if !al.is_finite() {
                al = alpha_rest();
            }
            if !(wp.abs() < 60.0) {
                wp = 0.0;
                thp = 0.0;
                bs = 0.0;
                drag_tq = 0.0;
            }
            l1 = 0.0;
            l2 = 0.0;
            l3 = 0.0;
            l4 = 0.0;
            r1 = 0.0;
            r2 = 0.0;
            r3 = 0.0;
            r4 = 0.0;
            self.ring_l.fill(0.0);
            self.ring_r.fill(0.0);
            hc_p = 1.0;
            self.blown += 1;
        }
        self.fz = [l1, l2, l3, l4, r1, r2, r3, r4, s1, s2, s3, s4];
        self.thp = thp;
        self.wp = wp;
        self.al = al;
        self.ald = ald;
        self.zb = zb;
        self.zbd = zbd;
        self.xi = xi;
        self.xid = xid;
        self.ze = ze;
        self.zed = zed;
        self.mwx = mwx;
        self.mwz = mwz;
        self.gn = gn;
        self.xg_p = xg_p;
        self.hc_p = hc_p;
        self.dp_in = dp_in;
        self.dp_out = dp_out;
        self.zs = zs;
        self.zcue = zcue;
        self.rp = rp;
        self.wj0 = wj0;
        self.t = t;
        self.drift = drift;
        self.drag_tq = drag_tq;
        self.bs = bs;
        self.vtf_s = vtf_s;
    }
}
/// Signed tip-sphere gap and contact normal for mirrored inner/outer groove walls.
fn gap_to(x: f64, hc: f64, d: f64, w: f64, r: f64) -> (f64, f64, f64) {
    let dist = (x + hc + w) * IS2 - d;
    if hc - dist * IS2 <= 0.0 {
        return (dist - r, IS2, IS2);
    }
    let xe = SQ2 * d - w;
    if x <= xe {
        return (hc - r, 0.0, 1.0);
    }
    let dx = x - xe;
    let l = (dx * dx + hc * hc).sqrt();
    (l - r, dx / l, hc / l)
}
