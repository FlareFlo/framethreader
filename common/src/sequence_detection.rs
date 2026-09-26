use crate::burst::BurstFile;
use indicatif::ProgressIterator;
use rayon::iter::ParallelBridge;
use rayon::iter::ParallelIterator;
use serde::{Deserialize, Deserializer};
use std::fs::File;
use std::io::{Read, Write};
use std::ops::Add;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use time::{Duration, PlainDateTime};

static ACCEPTED_IMAGE_EXTENSIONS: &[&str] = &["ARW", "HEIC", "JPG", "HEIF"];

#[derive(serde::Deserialize, Debug)]
struct ExifRaw {
    #[serde(rename = "CreateDate")]
    create_date: String,
    #[serde(
        rename = "SubSecTimeOriginal",
        deserialize_with = "deserialize_string_or_int"
    )]
    subsec: u16,
}

pub fn extract_exif_exiftool(path: &PathBuf) -> Option<BurstFile> {
    let mut head = File::open(path).unwrap();
    let mut buf = vec![0u8; 2usize.pow(16)];
    let n = head.read(buf.as_mut_slice()).unwrap_or(0);
    if n == 0 { return None; }

    let mut exiftool = Command::new("exiftool")
        .args(["-json", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = exiftool.stdin.as_mut().unwrap();
    stdin.write_all(&buf[..n]).unwrap();

    let res = exiftool.wait_with_output().unwrap();
    if res.stdout.is_empty() { return None; }
    let ser: Result<Vec<ExifRaw>, _> = serde_json::from_slice(&res.stdout);
    if let Ok(ser) = ser {
        if let Some(s) = ser.first() {
            return Some(BurstFile::new(path.clone(), &s.create_date, s.subsec));
        }
    }
    None
}

pub fn extract_exif_native(path: &PathBuf) -> Option<BurstFile> {
    let file = File::open(path).ok()?;
    let mut bufreader = std::io::BufReader::with_capacity(2_usize.pow(14), file);
    let exifreader = exif::Reader::new();
    let exif = exifreader.read_from_container(&mut bufreader).ok()?;

    let create_date = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .or_else(|| exif.get_field(exif::Tag::DateTimeDigitized, exif::In::PRIMARY))
        .or_else(|| exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY))?;
    
    // The date format is usually "YYYY:MM:DD HH:MM:SS"
    let create_date_str = match create_date.value {
        exif::Value::Ascii(ref vec) if !vec.is_empty() => {
            std::str::from_utf8(&vec[0]).unwrap_or("").trim_end_matches('\0').to_string()
        }
        _ => return None,
    };

    let subsec = exif.get_field(exif::Tag::SubSecTimeOriginal, exif::In::PRIMARY);
    let subsec_val = if let Some(sub) = subsec {
        match sub.value {
            exif::Value::Ascii(ref vec) if !vec.is_empty() => {
                let s = std::str::from_utf8(&vec[0]).unwrap_or("").trim_end_matches('\0');
                s.parse::<u16>().unwrap_or(0)
            }
            exif::Value::Short(ref vec) if !vec.is_empty() => vec[0] as u16,
            exif::Value::Long(ref vec) if !vec.is_empty() => vec[0] as u16,
            _ => 0,
        }
    } else {
        0
    };

    Some(BurstFile::new(path.clone(), &create_date_str, subsec_val))
}

pub fn scan_images(path: PathBuf) -> Vec<BurstFile> {
    let valid_files = path
        .read_dir()
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().unwrap().is_file()
                && ACCEPTED_IMAGE_EXTENSIONS.contains(
                    &e.path()
                        .extension()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_ascii_uppercase()
                        .as_str(),
                )
        })
        .collect::<Vec<_>>();

    let len = valid_files.len();
    set_total(len);
    set_current_task("Scanning all files");
    let mut all_files = valid_files
        .into_iter()
        .progress_count(len as _)
        .filter_map(|valid_file| {
            set_current_file(valid_file.path().display().to_string());
            
            // Swap this between extract_exif_native and extract_exif_exiftool
            let res = extract_exif_native(&valid_file.path());
            
            incr(1);
            res
        })
        .collect::<Vec<_>>();
    all_files.sort_unstable_by_key(|k| *k.created());
    all_files
}

pub fn detect_by_time(frames: Vec<BurstFile>, threshold: Duration) -> Vec<Vec<BurstFile>> {
    let mut bursts = vec![];
    let mut last_td = PlainDateTime::MIN;
    let mut current_burst = vec![];
    for frame in frames {
        let created_now = *frame.created();
        if last_td.add(threshold) >= *frame.created() {
            current_burst.push(frame);
        } else {
            bursts.push(current_burst);
            current_burst = vec![];
        }

        last_td = created_now;
    }
    bursts
}

// Stupid dumb fucking adapter that i dont see the reason to exist but i must use because exiftool does stupid fucking shit
fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrInt {
        Int(u16),
        String(String),
    }

    match StringOrInt::deserialize(deserializer)? {
        StringOrInt::Int(i) => Ok(i),
        StringOrInt::String(s) => s.parse::<u16>().map_err(serde::de::Error::custom),
    }
}

static CURRENT_PROGRESS: Mutex<ScanProgress> = Mutex::new(ScanProgress {
    total: 0,
    completed: 0,
    current_file: String::new(),
    current_task: String::new(),
});

#[derive(Debug, Default, Clone)]
pub struct ScanProgress {
    pub total: usize,
    pub completed: usize,
    pub current_task: String,
    pub current_file: String,
}

fn set_total(total: usize) {
    let mut t = CURRENT_PROGRESS.lock().unwrap();
    t.total = total;
    t.completed = 0;
}

fn incr(delta: usize) {
    let mut t = CURRENT_PROGRESS.lock().unwrap();
    t.completed = t.completed.add(delta).min(t.total);
}

fn set_current_file(file: impl ToString) {
    CURRENT_PROGRESS.lock().unwrap().current_file = file.to_string();
}

fn set_current_task(task: impl ToString) {
    CURRENT_PROGRESS.lock().unwrap().current_task = task.to_string();
}

pub fn get_progress() -> ScanProgress {
    CURRENT_PROGRESS.lock().unwrap().clone()
}

impl ScanProgress {
    pub fn progress_ratio(&self) -> f32 {
        self.completed as f32 / self.total as f32
    }
    pub fn complete(&self) -> bool {
        self.completed == self.total
    }
}
