use indicatif::{ProgressBar, ProgressIterator};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelBridge;
use rayon::iter::ParallelIterator;
use serde::{Deserialize, Deserializer};
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::ops::Add;
use std::path::Component::CurDir;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use time::format_description::well_known;
use time::macros::format_description;
use time::{Duration, PlainDateTime, Time};

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

#[derive(Debug)]
pub struct BurstFile {
    pub path: PathBuf,
    pub created: PlainDateTime,
}

pub fn scan_images(path: PathBuf) -> Vec<BurstFile> {
    let datefmt = format_description!("[year]:[month]:[day] [hour]:[minute]:[second]");

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
        .par_bridge()
        .map(|valid_file| {
            set_current_file(valid_file.path().display().to_string());
            let mut head = File::open(&valid_file.path()).unwrap();
            let mut buf = vec![0u8; 2usize.pow(16)];
            head.read_exact(buf.as_mut_slice()).unwrap();

            let mut exiftool = Command::new("exiftool")
                .args(["-json", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let stdin = exiftool.stdin.as_mut().unwrap();
            stdin.write_all(&buf).unwrap();

            let res = exiftool.wait_with_output().unwrap();
            // eprintln!("{}", String::from_utf8(res.stdout.clone()).unwrap());
            // fs::write("out.json", res.stdout.clone()).unwrap();
            let ser: Vec<ExifRaw> = serde_json::from_slice(&res.stdout).unwrap();
            let ser = &ser[0];
            incr(1);
            BurstFile {
                path: valid_file.path(),
                created: PlainDateTime::parse(&ser.create_date, &datefmt)
                    .unwrap()
                    .replace_microsecond(ser.subsec as u32 * 1000)
                    .unwrap(),
            }
        })
        .collect::<Vec<_>>();
    all_files.sort_unstable_by_key(|k| k.created);
    all_files
}

pub fn detect_by_time(frames: Vec<BurstFile>, threshold: Duration) -> Vec<Vec<BurstFile>> {
    let mut bursts = vec![];
    let mut last_td = PlainDateTime::MIN;
    let mut current_burst = vec![];
    for frame in frames {
        let created_now = frame.created;
        if last_td.add(threshold) >= frame.created {
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
