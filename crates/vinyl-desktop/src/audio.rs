//! The device callback owns the engine. UI communication is bounded and nonblocking.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::{
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread,
    time::Instant,
};
use vinyl_core::{Engine, Parameters, Record, Stylus, Telemetry};

pub enum Command {
    /// Sequences must increase so acknowledged record storage can be retired on the UI thread.
    Record {
        record: Arc<Record>,
        sequence: u64,
    },
    Parameters(Parameters),
    Stylus(Stylus),
    TrackingForce(f64),
    Antiskate(f64),
    Capacitance(f64),
    Motor(bool),
    Speed(f64),
    Cue(bool),
    CueTo(f64),
    Hand(bool, f64),
    PlatterHand(bool),
    PlatterMove(f64),
    Knock(f64),
}

#[derive(Clone, Copy)]
pub struct Snapshot {
    pub telemetry: Telemetry,
    pub sequence: u64,
    pub load: f32,
    pub peak: f32,
}

pub struct DeviceInfo {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: String,
}

pub struct AudioController {
    commands: SyncSender<Command>,
    snapshots: Receiver<Snapshot>,
    errors: Receiver<String>,
    quit: SyncSender<()>,
    worker: Option<thread::JoinHandle<()>>,
    info: DeviceInfo,
    // Retain the callback's current and queued records until telemetry acknowledges a swap.
    // The final Arc drop (tens of megabytes of wall data) therefore happens on this thread.
    records: Vec<(u64, Arc<Record>)>,
}

impl AudioController {
    pub fn start() -> Result<Self, String> {
        let (command_tx, command_rx) = mpsc::sync_channel(128);
        let (snapshot_tx, snapshot_rx) = mpsc::sync_channel(4);
        let (error_tx, error_rx) = mpsc::sync_channel(8);
        let (quit_tx, quit_rx) = mpsc::sync_channel(1);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("vinyl-device".into())
            .spawn(
                move || match open_device(command_rx, snapshot_tx, error_tx) {
                    Ok((stream, info)) => {
                        if ready_tx.send(Ok(info)).is_ok() {
                            let _ = quit_rx.recv();
                        }
                        drop(stream);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                },
            )
            .map_err(|e| format!("Could not start audio thread: {e}"))?;
        let info = match ready_rx.recv() {
            Ok(Ok(info)) => info,
            Ok(Err(error)) => {
                let _ = worker.join();
                return Err(error);
            }
            Err(_) => {
                let _ = worker.join();
                return Err("Audio device initialization stopped unexpectedly".into());
            }
        };
        Ok(Self {
            commands: command_tx,
            snapshots: snapshot_rx,
            errors: error_rx,
            quit: quit_tx,
            worker: Some(worker),
            info,
            records: Vec::new(),
        })
    }

    /// False means the bounded queue is full or the device thread has stopped.
    pub fn command(&mut self, command: Command) -> bool {
        let retained = match &command {
            Command::Record { record, sequence } => Some((*sequence, Arc::clone(record))),
            _ => None,
        };
        if self.commands.try_send(command).is_err() {
            return false;
        }
        if let Some(record) = retained {
            self.records.push(record);
        }
        true
    }

    pub fn latest_snapshot(&mut self) -> Option<Snapshot> {
        let mut latest = None;
        while let Ok(snapshot) = self.snapshots.try_recv() {
            latest = Some(snapshot);
        }
        if let Some(snapshot) = latest {
            self.records
                .retain(|(sequence, _)| *sequence >= snapshot.sequence);
        }
        latest
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }
    pub fn take_error(&self) -> Option<String> {
        self.errors.try_recv().ok()
    }
}

impl Drop for AudioController {
    fn drop(&mut self) {
        let _ = self.quit.try_send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn open_device(
    commands: Receiver<Command>,
    snapshots: SyncSender<Snapshot>,
    errors: SyncSender<String>,
) -> Result<(cpal::Stream, DeviceInfo), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No default audio output device is available")?;
    let preferred = device
        .supported_output_configs()
        .ok()
        .and_then(|mut configs| {
            configs
                .find(|config| {
                    config.channels() == 2
                        && config.sample_format() == SampleFormat::F32
                        && config.min_sample_rate().0 <= 48_000
                        && config.max_sample_rate().0 >= 48_000
                })
                .map(|config| config.with_sample_rate(cpal::SampleRate(48_000)))
        });
    let supported = match preferred {
        Some(supported) => supported,
        None => device
            .default_output_config()
            .map_err(|e| format!("Could not inspect audio output: {e}"))?,
    };
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    if config.channels == 0 {
        return Err("Audio output has no channels".into());
    }
    let info = DeviceInfo {
        name: device.name().unwrap_or_else(|_| "Default output".into()),
        sample_rate: config.sample_rate.0,
        channels: config.channels,
        format: format.to_string(),
    };
    let callback = Callback::new(
        config.sample_rate.0,
        config.channels as usize,
        commands,
        snapshots,
    );
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, callback, errors),
        SampleFormat::F64 => build::<f64>(&device, &config, callback, errors),
        SampleFormat::I8 => build::<i8>(&device, &config, callback, errors),
        SampleFormat::I16 => build::<i16>(&device, &config, callback, errors),
        SampleFormat::I32 => build::<i32>(&device, &config, callback, errors),
        SampleFormat::I64 => build::<i64>(&device, &config, callback, errors),
        SampleFormat::U8 => build::<u8>(&device, &config, callback, errors),
        SampleFormat::U16 => build::<u16>(&device, &config, callback, errors),
        SampleFormat::U32 => build::<u32>(&device, &config, callback, errors),
        SampleFormat::U64 => build::<u64>(&device, &config, callback, errors),
        _ => return Err(format!("Unsupported audio device sample format: {format}")),
    }
    .map_err(|e| format!("Could not open audio output: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("Could not start audio output: {e}"))?;
    Ok((stream, info))
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut callback: Callback,
    errors: SyncSender<String>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    device.build_output_stream(
        config,
        move |output: &mut [T], _: &cpal::OutputCallbackInfo| callback.render(output),
        move |error| {
            let _ = errors.try_send(format!("Audio output stopped: {error}"));
        },
        None,
    )
}

struct Callback {
    engine: Engine,
    commands: Receiver<Command>,
    snapshots: SyncSender<Snapshot>,
    channels: usize,
    sample_rate: u32,
    left: [f32; 256],
    right: [f32; 256],
    sequence: u64,
    snapshot_frames: usize,
    load_frames: usize,
    busy_seconds: f64,
    load: f32,
    peak: f32,
}

impl Callback {
    fn new(
        sample_rate: u32,
        channels: usize,
        commands: Receiver<Command>,
        snapshots: SyncSender<Snapshot>,
    ) -> Self {
        Self {
            engine: Engine::new(sample_rate as f64),
            commands,
            snapshots,
            channels,
            sample_rate,
            left: [0.0; 256],
            right: [0.0; 256],
            sequence: 0,
            snapshot_frames: 0,
            load_frames: 0,
            busy_seconds: 0.0,
            load: 0.0,
            peak: 0.0,
        }
    }

    fn render<T: SizedSample + FromSample<f32>>(&mut self, output: &mut [T]) {
        let started = Instant::now();
        // A command flood cannot starve audio processing.
        for _ in 0..128 {
            let Ok(command) = self.commands.try_recv() else {
                break;
            };
            match command {
                Command::Record { record, sequence } => {
                    let _ = self.engine.set_record(record);
                    self.sequence = sequence;
                }
                Command::Parameters(parameters) => self.engine.apply_parameters(parameters),
                Command::Stylus(stylus) => self.engine.set_stylus(stylus),
                Command::TrackingForce(grams) => self.engine.set_vtf(grams),
                Command::Antiskate(grams) => self.engine.set_antiskate(grams),
                Command::Capacitance(picofarads) => self.engine.set_capacitance(picofarads),
                Command::Motor(on) => self.engine.set_motor(on),
                Command::Speed(rpm) => self.engine.set_speed(rpm),
                Command::Cue(up) => self.engine.set_cue(up),
                Command::CueTo(angle) => self.engine.cue_to(angle),
                Command::Hand(on, angle) => self.engine.hand(on, angle),
                Command::PlatterHand(on) => self.engine.platter_hand(on),
                Command::PlatterMove(delta) => self.engine.platter_move(delta),
                Command::Knock(strength) => self.engine.knock(strength),
            }
        }
        let channels = self.channels;
        for block in output.chunks_mut(256 * channels) {
            let count = block.len() / channels;
            self.engine
                .process(&mut self.left[..count], &mut self.right[..count]);
            for (index, frame) in block.chunks_exact_mut(channels).enumerate() {
                let left = sanitize(self.left[index]);
                let right = sanitize(self.right[index]);
                self.peak = self.peak.max(left.abs()).max(right.abs());
                if channels == 1 {
                    frame[0] = T::from_sample((left + right) * 0.5);
                } else {
                    frame[0] = T::from_sample(left);
                    frame[1] = T::from_sample(right);
                    for sample in &mut frame[2..] {
                        *sample = T::EQUILIBRIUM;
                    }
                }
            }
        }
        let frames = output.len() / channels;
        self.busy_seconds += started.elapsed().as_secs_f64();
        self.load_frames += frames;
        self.snapshot_frames += frames;
        if self.load_frames >= self.sample_rate as usize / 2 {
            self.load =
                (self.busy_seconds * self.sample_rate as f64 / self.load_frames as f64) as f32;
            self.busy_seconds = 0.0;
            self.load_frames = 0;
        }
        if self.snapshot_frames >= self.sample_rate as usize / 60 {
            let snapshot = Snapshot {
                telemetry: self.engine.telemetry(),
                sequence: self.sequence,
                load: self.load,
                peak: self.peak,
            };
            let _ = self.snapshots.try_send(snapshot);
            self.snapshot_frames = 0;
            self.peak = 0.0;
        }
    }
}

fn sanitize(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

pub struct RenderInfo {
    pub frames: usize,
    pub sample_rate: u32,
    pub peak: f64,
    pub rms: f64,
    pub clipped_samples: usize,
    pub blown: u64,
}

/// Export the same stylus simulation without requiring an output device or a display.
/// Spin up, move to the first programme groove and let the needle settle before writing.
pub fn render_wav(
    record: Arc<Record>,
    path: &std::path::Path,
    seconds: f64,
) -> Result<RenderInfo, String> {
    use std::io::Write;
    use vinyl_core::geometry::{alpha_for_radius, groove_radius, stylus_xy};
    if !seconds.is_finite() || seconds <= 0.0 || seconds > 3600.0 {
        return Err("Render duration must be between zero and 3600 seconds".into());
    }
    let sample_rate = 48_000_u32;
    let frames = (seconds * sample_rate as f64).round() as usize;
    let bytes = (frames as u32)
        .checked_mul(4)
        .ok_or("WAV file is too large")?;
    let theta = record
        .tracks
        .first()
        .map(|track| track.theta0 + 0.1)
        .unwrap_or(0.0);
    let angle = alpha_for_radius(groove_radius(&record.r_tab, theta));
    let position = stylus_xy(angle);
    let mut engine = Engine::new(sample_rate as f64);
    let _ = engine.set_record(record);
    engine.set_motor(true);
    let mut left = [0.0_f32; 256];
    let mut right = [0.0_f32; 256];
    for _ in 0..(sample_rate as usize * 2).div_ceil(256) {
        engine.process(&mut left, &mut right);
    }
    engine.al = angle;
    engine.thp = theta - position[1].atan2(position[0]);
    engine.zb = 0.3e-3;
    engine.zcue = 0.3e-3;
    engine.set_cue(false);
    for _ in 0..(sample_rate as usize / 2).div_ceil(256) {
        engine.process(&mut left, &mut right);
    }
    let file =
        std::fs::File::create(path).map_err(|e| format!("Could not create WAV export: {e}"))?;
    let mut writer = std::io::BufWriter::new(file);
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + bytes).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&1_u16.to_le_bytes());
    header.extend_from_slice(&2_u16.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    header.extend_from_slice(&4_u16.to_le_bytes());
    header.extend_from_slice(&16_u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&bytes.to_le_bytes());
    writer
        .write_all(&header)
        .map_err(|e| format!("Could not write WAV header: {e}"))?;
    let mut pcm = [0_u8; 1024];
    let mut sum = 0.0;
    let mut peak = 0.0_f64;
    let mut clipped_samples = 0;
    let mut remaining = frames;
    while remaining > 0 {
        let count = remaining.min(left.len());
        engine.process(&mut left[..count], &mut right[..count]);
        for index in 0..count {
            for (channel, sample) in [left[index], right[index]].into_iter().enumerate() {
                if !sample.is_finite() {
                    return Err("Physical simulation produced a non-finite sample".into());
                }
                if sample.abs() > 1.0 {
                    clipped_samples += 1;
                }
                let value = sanitize(sample) as f64;
                sum += value * value;
                peak = peak.max(value.abs());
                let integer = (value * 32767.0).round() as i16;
                let offset = index * 4 + channel * 2;
                pcm[offset..offset + 2].copy_from_slice(&integer.to_le_bytes());
            }
        }
        writer
            .write_all(&pcm[..count * 4])
            .map_err(|e| format!("Could not write WAV audio: {e}"))?;
        remaining -= count;
    }
    writer
        .flush()
        .map_err(|e| format!("Could not finish WAV export: {e}"))?;
    Ok(RenderInfo {
        frames,
        sample_rate,
        peak,
        rms: (sum / (frames * 2) as f64).sqrt(),
        clipped_samples,
        blown: engine.blown,
    })
}
