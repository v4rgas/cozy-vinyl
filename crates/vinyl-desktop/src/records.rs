//! Loading, decoding and pressing happen on a worker, never on the audio callback.
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};
use vinyl_core::press::{make_vis, Visual};
use vinyl_core::{cut, press, AudioTrack, Condition, CutOptions, Record};

pub struct RecordEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub by: &'static str,
    pub credit: &'static str,
    pub licence: &'static str,
    pub url: &'static str,
}

pub fn catalog() -> Vec<RecordEntry> {
    vec![
        RecordEntry { id:"house", title:"Demo EP", by:"Synthesised", credit:"Synthesised by Vinyl, with a test band", licence:"", url:"" },
        RecordEntry { id:"summer-air", title:"Summer Air", by:"HoliznaCC0", credit:"HoliznaCC0", licence:"CC0", url:"https://freemusicarchive.org/music/holiznacc0/summer-air-lo-fi/" },
        RecordEntry { id:"rhapsody", title:"Rhapsody in Blue (1924)", by:"Gershwin", credit:"George Gershwin, piano, with Paul Whiteman’s Concert Orchestra. Victor, 10 June 1924", licence:"Public domain", url:"https://www.loc.gov/item/jukebox-70342/" },
        RecordEntry { id:"moonlight", title:"Moonlight Sonata", by:"Beethoven", credit:"Paul Pitman, piano, for Musopen", licence:"Public domain", url:"https://commons.wikimedia.org/wiki/File:Ludwig_van_Beethoven_-_sonata_no._14_in_c_sharp_minor_%27moonlight%27,_op._27_no._2_-_i._adagio_sostenuto.ogg" },
        RecordEntry { id:"gymnopedie", title:"Gymnopédie No. 1", by:"Satie", credit:"Robin Alciatore, piano, for Musopen", licence:"Public domain", url:"https://commons.wikimedia.org/wiki/File:Erik_Satie_-_gymnopedies_-_la_1_ere._lent_et_douloureux.ogg" },
        RecordEntry { id:"nocturne", title:"Nocturne Op. 9 No. 2", by:"Chopin", credit:"Frank Lévy, piano, for Musopen", licence:"Public domain", url:"https://commons.wikimedia.org/wiki/File:Chopin_-_Nocturne_No._2_in_E-flat_major,_Op._9_No._2_(Frank_Levy).flac" },
        RecordEntry { id:"clair-de-lune", title:"Clair de lune", by:"Debussy", credit:"Laurens Goedhart, piano, 2011", licence:"Public domain", url:"https://commons.wikimedia.org/wiki/File:Clair_de_lune_(Claude_Debussy)_Suite_bergamasque.ogg" },
        RecordEntry { id:"mountain-king", title:"Hall of the Mountain King", by:"Grieg", credit:"Musopen Symphony", licence:"Public domain", url:"https://commons.wikimedia.org/wiki/File:Grieg_-_Peer_Gynt_Suite_No._1,_Op._46_-_IV._In_the_Hall_of_the_Mountain_King_(Musopen_Symphony).flac" },
        RecordEntry { id:"1812", title:"1812 Overture", by:"Tchaikovsky", credit:"US Marine Band, live at Wolf Trap, 2019", licence:"Public domain", url:"https://commons.wikimedia.org/wiki/File:1812_Overture_-_United_States_Marine_Band.opus" },
        RecordEntry { id:"toccata", title:"Toccata and Fugue", by:"Bach", credit:"Ashtar Moïra, organ", licence:"Public domain", url:"https://commons.wikimedia.org/wiki/File:Toccata_et_Fugue_BWV565.ogg" },
        RecordEntry { id:"summer-presto", title:"Summer: Presto", by:"Vivaldi", credit:"John Harrison, violin, with the Wichita State University Chamber Players", licence:"CC BY-SA 4.0", url:"https://commons.wikimedia.org/wiki/File:Vivaldi_-_Four_Seasons_2_Summer_mvt_3_Presto_-_John_Harrison_violin.oga" },
    ]
}

pub struct LoadedRecord {
    pub record: Arc<Record>,
    pub visual: Visual,
    pub title: String,
    pub credit: String,
}

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    fs: u32,
    walls: String,
    gain: f64,
    n: usize,
    rec: Record,
    #[serde(rename = "rTab")]
    r_tab: Vec<f64>,
    press: Pressing,
    vis: VisualManifest,
}
#[derive(Deserialize)]
struct Pressing {
    seed: u32,
    cond: Condition,
}
#[derive(Deserialize)]
struct VisualManifest {
    file: String,
    cols: usize,
    rows: usize,
}

/// Read the shipped lossless wall slopes, reconstruct metres, then apply the pressing.
/// `root` is the repository/bundle root containing `assets/`.
pub fn load_builtin(
    root: &Path,
    id: &str,
    condition: Option<Condition>,
    seed: u32,
) -> Result<LoadedRecord, String> {
    let entry = catalog()
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| format!("Unknown record: {id}"))?;
    let dir = if id == "house" {
        root.join("assets")
    } else {
        root.join("assets").join(id)
    };
    let bytes = fs::read(dir.join("record.json"))
        .map_err(|e| format!("Could not read {}: {e}", dir.display()))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid record manifest: {e}"))?;
    if manifest.version != 2
        || !manifest.gain.is_finite()
        || manifest.gain <= 0.0
        || manifest.n == 0
    {
        return Err("Unsupported or invalid groove manifest".into());
    }
    let decoded = decode_audio(&dir.join(&manifest.walls), Some(manifest.n))?;
    if decoded.sample_rate != manifest.fs
        || decoded.channels != 2
        || decoded.bits != Some(16)
        || decoded.left.len() != manifest.n
    {
        return Err("The groove FLAC does not match its manifest".into());
    }
    let mut rec = manifest.rec;
    rec.r_tab = manifest.r_tab;
    rec.wall_in = decoded.left;
    rec.wall_out = decoded.right;
    rec.n = manifest.n;
    if rec.r_tab.len() < 2 || rec.wall_out.len() != rec.n || rec.fs != manifest.fs as f64 {
        return Err("Incomplete groove geometry".into());
    }
    for wall in [&mut rec.wall_in, &mut rec.wall_out] {
        let mut displacement = 0.0;
        for value in wall {
            let integer = (*value as f64 * 32768.0).round();
            displacement += integer * manifest.gain;
            *value = displacement as f32;
        }
    }
    let changed = condition.is_some();
    press(
        &mut rec,
        condition.unwrap_or(manifest.press.cond),
        if changed { seed } else { manifest.press.seed },
    );
    let visual = if changed {
        make_vis(&rec, 512)
    } else {
        let data = fs::read(dir.join(&manifest.vis.file))
            .map_err(|e| format!("Could not read groove visualization: {e}"))?;
        if manifest.vis.cols.checked_mul(manifest.vis.rows) != Some(data.len()) {
            return Err("Invalid groove visualization dimensions".into());
        }
        Visual {
            data,
            cols: manifest.vis.cols,
            rows: manifest.vis.rows,
        }
    };
    Ok(LoadedRecord {
        record: Arc::new(rec),
        visual,
        title: entry.title.into(),
        credit: [entry.credit, entry.licence]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
    })
}

pub fn load_files(
    paths: &[PathBuf],
    condition: Condition,
    seed: u32,
) -> Result<LoadedRecord, String> {
    if paths.is_empty() {
        return Err("Choose at least one audio file".into());
    }
    let mut tracks = Vec::with_capacity(paths.len());
    for path in paths {
        let decoded = decode_audio(path, None)?;
        let title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        tracks.push(AudioTrack {
            left: resample(&decoded.left, decoded.sample_rate, 48_000),
            right: resample(&decoded.right, decoded.sample_rate, 48_000),
            title,
        });
    }
    let title = if tracks.len() == 1 {
        tracks[0].title.clone()
    } else {
        "Your audio".into()
    };
    let mut record = cut(&tracks, 48_000.0, CutOptions::default());
    press(&mut record, condition, seed);
    let visual = make_vis(&record, 512);
    Ok(LoadedRecord {
        record: Arc::new(record),
        visual,
        title,
        credit: "Your own audio".into(),
    })
}

pub struct DecodedAudio {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
    pub bits: Option<u32>,
}

/// Symphonia supports FLAC, MP3, WAV, AIFF, AAC, ALAC and Vorbis.
/// Opus is deliberately surfaced as a decoder error rather than pretending to support it.
pub fn decode_audio(path: &Path, expected_frames: Option<usize>) -> Result<DecodedAudio, String> {
    let file =
        fs::File::open(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Could not recognize {}: {e}", path.display()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or("This file has no audio track")?;
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("Unknown audio sample rate")?;
    let channels = track
        .codec_params
        .channels
        .ok_or("Unknown channel layout")?
        .count();
    let bits = track.codec_params.bits_per_sample;
    if sample_rate == 0 || channels == 0 {
        return Err("Invalid audio format".into());
    }
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Cannot decode {}: {e}", path.display()))?;
    let capacity = expected_frames.unwrap_or_else(|| {
        track
            .codec_params
            .n_frames
            .unwrap_or(0)
            .min(48_000 * 60 * 30) as usize
    });
    let mut left = Vec::with_capacity(capacity);
    let mut right = Vec::with_capacity(capacity);
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("Could not read audio packets: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| format!("Could not decode audio packet: {e}"))?;
        if decoded.spec().rate != sample_rate || decoded.spec().channels.count() != channels {
            return Err("Audio format changed within the file".into());
        }
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        samples.copy_interleaved_ref(decoded);
        for frame in samples.samples().chunks_exact(channels) {
            let l = frame[0];
            let r = frame[if channels > 1 { 1 } else { 0 }];
            if !l.is_finite() || !r.is_finite() {
                return Err("Audio contains non-finite samples".into());
            }
            left.push(l);
            right.push(r);
        }
        if expected_frames.is_some_and(|expected| left.len() > expected) {
            return Err("Groove audio is longer than its manifest".into());
        }
    }
    if left.is_empty() {
        return Err("This audio file is empty".into());
    }
    Ok(DecodedAudio {
        left,
        right,
        sample_rate,
        channels,
        bits,
    })
}

/// Windowed sinc conversion preserves treble and rejects aliases before the lathe.
pub(crate) fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to {
        return input.to_vec();
    }
    let count = (input.len() as f64 * to as f64 / from as f64).round() as usize;
    let cutoff = (to as f64 / from as f64).min(1.0) * 0.94;
    let half = 24;
    (0..count)
        .map(|i| {
            let position = i as f64 * from as f64 / to as f64;
            let center = position.floor() as isize;
            let mut sum = 0.0;
            let mut weight = 0.0;
            for j in center - half + 1..=center + half {
                let d = j as f64 - position;
                let phase = std::f64::consts::PI * cutoff * d;
                let sinc = if phase.abs() < 1e-12 {
                    cutoff
                } else {
                    cutoff * phase.sin() / phase
                };
                let window = 0.5 + 0.5 * (std::f64::consts::PI * d / half as f64).cos();
                let coefficient = sinc * window;
                let index = j.clamp(0, input.len() as isize - 1) as usize;
                sum += input[index] as f64 * coefficient;
                weight += coefficient;
            }
            (sum / weight) as f32
        })
        .collect()
}
