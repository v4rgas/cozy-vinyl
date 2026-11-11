//! Finite scanning-radius contact: circle dilation over an 8× sinc reconstruction.
use crate::{
    dsp::{make_polyphase, TAU},
    geometry::groove_radius,
    Record,
};
const UP: usize = 8;
const UT: usize = 32;
const BS: usize = 1024;
const SLOTS: usize = 32;
const DB: usize = 2 * BS;
pub struct GrooveReader {
    up: Vec<f32>,
    r_scan: f64,
    tag: [isize; SLOTS],
    blk: Vec<Vec<f32>>,
    fine: Vec<f32>,
    cap: [f32; 601],
    pub computed: u64,
}
fn upsample(src: &[f32], t0: isize, n: usize, up: &[f32], fine: &mut [f32], fo: usize) {
    for p in 0..UP {
        let mut a = 0.0;
        let co = p * UT;
        for t in 0..UT {
            let k = t0 + t as isize;
            if k >= 0 && (k as usize) < n {
                a += up[co + t] as f64 * src[k as usize] as f64;
            }
        }
        fine[fo + p] = a as f32;
    }
}
fn roll(fine: &[f32], c: usize, cap: &[f32], m: usize) -> f64 {
    let mut best = -1.0;
    let mut bk = 0_isize;
    let m = m as isize;
    for k in -m..=m {
        let v = fine[(c as isize + k) as usize] as f64 + cap[(k + m) as usize] as f64;
        if v > best {
            best = v;
            bk = k;
        }
    }
    if bk > -m && bk < m {
        let y0 = fine[(c as isize + bk - 1) as usize] as f64 + cap[(bk - 1 + m) as usize] as f64;
        let y2 = fine[(c as isize + bk + 1) as usize] as f64 + cap[(bk + 1 + m) as usize] as f64;
        let den = y0 - 2.0 * best + y2;
        if den < 0.0 {
            best -= 0.125 * (y2 - y0) * (y2 - y0) / den;
        }
    }
    best
}
impl GrooveReader {
    pub fn new() -> Self {
        Self {
            up: make_polyphase(UT, UP, 1.0, 9.0),
            r_scan: 18e-6,
            tag: [-1; SLOTS],
            blk: (0..SLOTS).map(|_| vec![0.0; 2 * DB]).collect(),
            fine: vec![0.0; (BS + 80) * UP],
            cap: [0.0; 601],
            computed: 0,
        }
    }
    pub fn reset(&mut self) {
        self.tag.fill(-1);
    }
    pub fn set_scan(&mut self, r: f64) {
        if r != self.r_scan {
            self.r_scan = r;
            self.reset();
        }
    }
    pub fn fill(
        &mut self,
        rec: &Record,
        j0: isize,
        len: usize,
        out_in: &mut [f32],
        out_out: &mut [f32],
    ) {
        let mut j = j0;
        let mut o = 0;
        while o < len {
            if j < 0 || j >= 2 * rec.n as isize {
                out_in[o] = 0.0;
                out_out[o] = 0.0;
                j += 1;
                o += 1;
                continue;
            }
            let b = j as usize / DB;
            let s = b & (SLOTS - 1);
            if self.tag[s] != b as isize {
                self.compute(rec, b, s);
                self.tag[s] = b as isize;
            }
            let base = j as usize - b * DB;
            let cnt = (len - o).min(DB - base);
            out_in[o..o + cnt].copy_from_slice(&self.blk[s][base..base + cnt]);
            out_out[o..o + cnt].copy_from_slice(&self.blk[s][DB + base..DB + base + cnt]);
            j += cnt as isize;
            o += cnt;
        }
    }
    fn compute(&mut self, rec: &Record, b: usize, slot: usize) {
        let i0 = b * BS;
        let r = groove_radius(&rec.r_tab, (i0 + BS / 2) as f64 * TAU / rec.spr);
        let h = TAU * r / rec.spr / UP as f64;
        let rs = self.r_scan;
        let m = (rs / h).ceil().min(300.0) as usize;
        for k in -(m as isize)..=m as isize {
            let kf = k as f64;
            self.cap[(k + m as isize) as usize] =
                ((rs * rs - kf * h * kf * h).max(0.0).sqrt() - rs) as f32;
        }
        let margin = (m as f64 / UP as f64).ceil() as usize + 2;
        let ia = i0 as isize - margin as isize;
        let cnt = BS + 2 * margin;
        // M <= 300 means margin <= 40; the maximum reconstruction is preallocated.
        // No cache miss or stylus change allocates in the audio callback.
        debug_assert!(cnt * UP <= self.fine.len());
        for wall in 0..2 {
            let src = if wall == 0 {
                &rec.wall_in
            } else {
                &rec.wall_out
            };
            for q in 0..cnt {
                upsample(
                    src,
                    ia + q as isize - UT as isize / 2 + 1,
                    rec.n,
                    &self.up,
                    &mut self.fine,
                    q * UP,
                );
            }
            let oo = wall * DB;
            for jj in 0..DB {
                self.blk[slot][oo + jj] = roll(
                    &self.fine,
                    (margin + (jj >> 1)) * UP + (jj & 1) * (UP / 2),
                    &self.cap,
                    m,
                ) as f32;
            }
        }
        self.computed += 1;
    }
}
