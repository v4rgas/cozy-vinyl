//! Two curated, numbered CC0 recording sets; cache original audio, cut one track at a time.
use crate::records::{decode_audio, resample, LoadedRecord};
use reqwest::{blocking::Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use vinyl_core::{cut, press, AudioTrack, Condition, CutOptions};

#[derive(Clone, Copy)]
pub struct Album {
    pub id: &'static str,
    pub title: &'static str,
    pub by: &'static str,
    pub category: &'static str,
    pub count: usize,
}

pub const ALBUMS: &[Album] = &[
    Album {
        id: "goldberg",
        title: "Open Goldberg Variations",
        by: "Kimiko Ishizaka",
        category: "The Open Goldberg Variations by Kimiko Ishizaka (album)",
        count: 32,
    },
    Album {
        id: "clavier",
        title: "Well-Tempered Clavier, Book I",
        by: "Kimiko Ishizaka",
        category: "Audio files of Open Well-Tempered Clavier, Book 1",
        count: 48,
    },
];

pub fn catalog() -> &'static [Album] {
    ALBUMS
}

pub fn album_source(album: &Album) -> String {
    let mut url = Url::parse("https://commons.wikimedia.org/wiki/").expect("constant URL");
    url.path_segments_mut()
        .expect("https URL")
        .push(&format!("Category:{}", album.category));
    url.to_string()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Track {
    pub number: usize,
    pub title: String,
    pub url: String,
    pub source: String,
    pub album_id: String,
    pub performer: String,
    pub license: String,
    pub mime: String,
    pub duration: f64,
    pub size: u64,
}

#[derive(Serialize, Deserialize)]
struct CachedAlbum {
    version: u32,
    album_id: String,
    tracks: Vec<Track>,
}

fn client() -> Result<Client, String> {
    Client::builder()
        .user_agent("CozyVinyl/0.1 (https://github.com/v4rgas/cozy-vinyl; public CC0 recordings)")
        .timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Could not initialize the Commons connection: {e}"))
}

pub fn load_album(album: &Album, cache: &Path) -> Result<Vec<Track>, String> {
    let canonical = ALBUMS
        .iter()
        .find(|known| known.id == album.id)
        .ok_or("Unknown Commons album")?;
    let manifest_path = cache.join(format!("commons-{}.json", canonical.id));
    if let Ok(bytes) = fs::read(&manifest_path) {
        if let Ok(cached) = serde_json::from_slice::<CachedAlbum>(&bytes) {
            if cached.version == 1
                && cached.album_id == canonical.id
                && complete(canonical, &cached.tracks)
            {
                return Ok(cached.tracks);
            }
        }
    }
    let connection = client()?;
    let mut continuation = BTreeMap::<String, String>::new();
    let mut tracks = BTreeMap::new();
    loop {
        let mut params = vec![
            ("action".to_owned(), "query".to_owned()),
            ("format".into(), "json".into()),
            ("formatversion".into(), "2".into()),
            ("generator".into(), "categorymembers".into()),
            (
                "gcmtitle".into(),
                format!("Category:{}", canonical.category),
            ),
            ("gcmtype".into(), "file".into()),
            ("gcmlimit".into(), "50".into()),
            ("prop".into(), "imageinfo".into()),
            ("iiprop".into(), "url|mime|size|extmetadata".into()),
        ];
        params.extend(
            continuation
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        let response = connection
            .get("https://commons.wikimedia.org/w/api.php")
            .query(&params)
            .send()
            .map_err(|e| format!("Commons is unavailable. Cached tracks remain playable. {e}"))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err("Commons is unavailable. Try the album again.".into());
        }
        let data: Value = response
            .json()
            .map_err(|e| format!("Could not read the Commons album listing: {e}"))?;
        if data.get("error").is_some() {
            return Err("Commons could not list this album. Try again later.".into());
        }
        if let Some(pages) = data.pointer("/query/pages").and_then(Value::as_array) {
            for page in pages {
                let Some(filename) = page.get("title").and_then(Value::as_str) else {
                    continue;
                };
                let Some((number, title)) = parse_filename(canonical.id, filename) else {
                    continue;
                };
                let Some(info) = page.pointer("/imageinfo/0") else {
                    continue;
                };
                let track = Track {
                    number,
                    title: title.into(),
                    url: value_string(info, "url"),
                    source: description_url(filename),
                    album_id: canonical.id.into(),
                    performer: canonical.by.into(),
                    license: info
                        .pointer("/extmetadata/License/value")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    mime: value_string(info, "mime"),
                    duration: info.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
                    size: info.get("size").and_then(Value::as_u64).unwrap_or(u64::MAX),
                };
                if eligible(&track) {
                    tracks.insert(number, track);
                }
            }
        }
        let Some(next) = data.get("continue").and_then(Value::as_object) else {
            break;
        };
        let next: BTreeMap<String, String> = next
            .iter()
            .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.into())))
            .collect();
        if next.is_empty() || next == continuation {
            return Err("Commons returned an invalid album continuation".into());
        }
        continuation = next;
    }
    let tracks: Vec<_> = tracks.into_values().collect();
    if !complete(canonical, &tracks) {
        return Err("Commons no longer lists the complete CC0 album. Choose another album.".into());
    }
    let cached = CachedAlbum {
        version: 1,
        album_id: canonical.id.into(),
        tracks,
    };
    let bytes = serde_json::to_vec(&cached)
        .map_err(|e| format!("Could not encode the album cache: {e}"))?;
    atomic_write(&manifest_path, &bytes)?;
    Ok(cached.tracks)
}

fn complete(album: &Album, tracks: &[Track]) -> bool {
    tracks.len() == album.count
        && tracks.iter().enumerate().all(|(index, track)| {
            track.number == index + 1
                && track.album_id == album.id
                && track.performer == album.by
                && eligible(track)
        })
}

fn eligible(track: &Track) -> bool {
    let valid_url = Url::parse(&track.url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("upload.wikimedia.org")
            && url.username().is_empty()
            && url.password().is_none()
    });
    track.license == "cc0"
        && matches!(track.mime.as_str(), "audio/ogg" | "application/ogg")
        && track.duration.is_finite()
        && track.duration > 0.0
        && track.duration <= 900.0
        && track.size > 0
        && track.size <= 40_000_000
        && valid_url
}

fn parse_filename<'a>(album_id: &str, filename: &'a str) -> Option<(usize, &'a str)> {
    let (prefix, separator) = match album_id {
        "goldberg" => ("File:Kimiko Ishizaka - ", " - "),
        "clavier" => (
            "File:Kimiko Ishizaka - Bach - Well-Tempered Clavier, Book 1 - ",
            " ",
        ),
        _ => return None,
    };
    let body = filename.strip_prefix(prefix)?.strip_suffix(".ogg")?;
    let (number, title) = body.split_once(separator)?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) || title.is_empty() {
        return None;
    }
    Some((number.parse().ok()?, title))
}

fn description_url(filename: &str) -> String {
    let mut url = Url::parse("https://commons.wikimedia.org/wiki/").expect("constant URL");
    url.path_segments_mut().expect("https URL").push(filename);
    url.to_string()
}

fn value_string(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").into()
}

pub fn prepare_track(
    track: &Track,
    cache: &Path,
    condition: Condition,
    seed: u32,
) -> Result<LoadedRecord, String> {
    let album = ALBUMS
        .iter()
        .find(|album| album.id == track.album_id)
        .ok_or("Unknown Commons album")?;
    if !eligible(track)
        || track.number == 0
        || track.number > album.count
        || track.performer != album.by
    {
        return Err("This recording does not meet the CC0 album requirements".into());
    }
    let path = cache
        .join("commons")
        .join(album.id)
        .join(format!("{:02}.ogg", track.number));
    if !path.is_file() {
        let response = client()?
            .get(&track.url)
            .send()
            .map_err(|e| format!("Could not download {}: {e}", track.title))?;
        if response.status() != reqwest::StatusCode::OK
            || response.url().scheme() != "https"
            || response.url().host_str() != Some("upload.wikimedia.org")
        {
            return Err("Could not download this Commons recording. Select it to retry.".into());
        }
        if response
            .content_length()
            .is_some_and(|length| length > 40_000_000)
        {
            return Err("This recording exceeds the 40 MB limit".into());
        }
        let mut bytes = Vec::new();
        response
            .take(40_000_001)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("The track download was interrupted: {e}"))?;
        if bytes.is_empty() || bytes.len() > 40_000_000 || bytes.len() as u64 != track.size {
            return Err("The downloaded recording does not match the Commons listing".into());
        }
        atomic_write(&path, &bytes)?;
    }
    let decoded = decode_audio(&path, None)?;
    if decoded.left.len() as f64 / decoded.sample_rate as f64 > 900.0 {
        return Err("This track is too long for one record".into());
    }
    let tracks = [AudioTrack {
        left: resample(&decoded.left, decoded.sample_rate, 48_000),
        right: resample(&decoded.right, decoded.sample_rate, 48_000),
        title: track.title.clone(),
    }];
    let mut record = cut(&tracks, 48_000.0, CutOptions::default());
    if record.truncated {
        return Err("This track does not fit on one record. Choose another track.".into());
    }
    press(&mut record, condition, seed);
    let visual = vinyl_core::press::make_vis(&record, 512);
    Ok(LoadedRecord {
        record: Arc::new(record),
        visual,
        title: track.title.clone(),
        credit: format!("{} · CC0", track.performer),
    })
}

static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid cache path")?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Could not create the local download cache: {e}"))?;
    let temporary = path.with_extension(format!(
        "{}.{}.part",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temporary, bytes).map_err(|e| format!("Could not save the download: {e}"))?;
    fs::rename(&temporary, path).map_err(|e| format!("Could not finish saving the download: {e}"))
}
