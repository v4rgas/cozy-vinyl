//! Disc and Baerwald nine-inch tonearm geometry, metres and radians.
use crate::dsp::TAU;
use std::f64::consts::PI;
pub const RPM33: f64 = 100.0 / 3.0;
pub const W33: f64 = TAU * RPM33 / 60.0;
pub const REV_SECONDS: f64 = 60.0 / RPM33;
pub const R_DISC: f64 = 0.1508;
pub const R_LEADIN: f64 = 0.1490;
pub const R_PROGRAM: f64 = 0.1463;
pub const R_PROGRAM_MIN: f64 = 0.0603;
pub const R_LOCK: f64 = 0.0532;
pub const R_LABEL: f64 = 0.050;
pub const GROOVE_W: f64 = 30e-6;
pub const LAND_MIN: f64 = 15e-6;
pub const TAB_PER_REV: usize = 64;
pub const ARM_LE: f64 = 0.2286;
pub const PIVOT_ANG: f64 = 27.0 * PI / 180.0;
// Rounded once to f64 from the equations below, as in the original module constants.
pub const ARM_D: f64 = 0.2104247133774928;
pub const ARM_BETA: f64 = 0.4211307318547522;
pub const PIVOT_X: f64 = 0.18748979246981343;
pub const PIVOT_Y: f64 = 0.09553082078379885;
pub const ALPHA_REST: f64 = 4.413917284545916;
pub const ALPHA_MIN: f64 = 3.8206814625910837;
pub fn arm_d() -> f64 {
    ARM_D
}
pub fn arm_beta() -> f64 {
    ARM_BETA
}
pub fn pivot_x() -> f64 {
    PIVOT_X
}
pub fn pivot_y() -> f64 {
    PIVOT_Y
}
pub fn alpha_for_radius(r: f64) -> f64 {
    let d = arm_d();
    let c = (d * d + ARM_LE * ARM_LE - r * r) / (2.0 * d * ARM_LE);
    PIVOT_ANG + PI + c.clamp(-1.0, 1.0).acos()
}
pub fn alpha_rest() -> f64 {
    ALPHA_REST
}
pub fn alpha_min() -> f64 {
    ALPHA_MIN
}
pub fn stylus_xy(alpha: f64) -> [f64; 2] {
    [
        pivot_x() + ARM_LE * alpha.cos(),
        pivot_y() + ARM_LE * alpha.sin(),
    ]
}
pub fn groove_radius(tab: &[f64], theta: f64) -> f64 {
    let x = theta * (TAB_PER_REV as f64 / TAU);
    let n = tab.len();
    if n == 0 {
        return R_LEADIN;
    }
    if x <= 0.0 {
        return tab[0];
    }
    if x >= (n - 1) as f64 {
        return tab[n - 1];
    }
    let i = x.floor() as usize;
    let f = x - i as f64;
    let p1 = tab[i];
    let p2 = tab[i + 1];
    let p0 = if i > 0 { tab[i - 1] } else { 2.0 * p1 - p2 };
    let p3 = if i + 2 < n { tab[i + 2] } else { 2.0 * p2 - p1 };
    p1 + 0.5
        * f
        * (p2 - p0 + f * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + f * (3.0 * (p1 - p2) + p3 - p0)))
}
pub fn theta_for_radius(tab: &[f64], r: f64) -> f64 {
    if tab.is_empty() {
        return 0.0;
    }
    let (mut lo, mut hi) = (0, tab.len() - 1);
    if r >= tab[0] {
        return 0.0;
    }
    if r <= tab[hi] {
        return hi as f64 * TAU / TAB_PER_REV as f64;
    }
    while hi - lo > 1 {
        let m = (lo + hi) >> 1;
        if tab[m] > r {
            lo = m;
        } else {
            hi = m;
        }
    }
    let d = tab[lo] - tab[hi];
    (lo as f64 + if d > 0.0 { (tab[lo] - r) / d } else { 0.0 }) * TAU / TAB_PER_REV as f64
}
