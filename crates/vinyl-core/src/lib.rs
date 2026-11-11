//! Physical vinyl mastering and playback, in SI units.
//! Audio becomes 45/45 groove wall displacement before a finite stylus reads it.
pub mod cut;
pub mod demo;
pub mod dsp;
pub mod engine;
pub mod geometry;
mod groove;
pub mod press;

pub use cut::{cut, AudioTrack, CutOptions};
pub use engine::{Engine, Parameters, Stylus, Telemetry};
pub use press::{press, Condition, Scratch, Warp};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackMark {
    pub title: String,
    pub theta0: f64,
    pub theta1: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub fs: f64,
    pub spr: f64,
    pub n: usize,
    #[serde(default, alias = "wallIn")]
    pub wall_in: Vec<f32>,
    #[serde(default, alias = "wallOut")]
    pub wall_out: Vec<f32>,
    #[serde(default, alias = "rTab")]
    pub r_tab: Vec<f64>,
    #[serde(alias = "thetaEnd")]
    pub theta_end: f64,
    #[serde(alias = "thetaLock")]
    pub theta_lock: f64,
    #[serde(alias = "W")]
    pub w: f64,
    #[serde(alias = "Vfs")]
    pub vfs: f64,
    #[serde(alias = "deEssDb")]
    pub de_ess_db: f64,
    pub truncated: bool,
    pub tracks: Vec<TrackMark>,
    #[serde(default)]
    pub ecc: [f64; 2],
    #[serde(default)]
    pub warp: Vec<Warp>,
    #[serde(default)]
    pub scratches: Vec<Scratch>,
    #[serde(default)]
    pub condition: Condition,
}

/// Synthesize, cut and press the original three-track demonstration EP.
pub fn demo_record(sample_rate: f64) -> Record {
    let mut record = cut(
        &demo::demo_tracks(sample_rate),
        sample_rate,
        CutOptions::default(),
    );
    press(&mut record, Condition::default(), 7);
    record
}
