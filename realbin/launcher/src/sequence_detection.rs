use time::macros::format_description;
use time::{PlainDateTime, Time};
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::ops::Add;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use indicatif::{ProgressBar, ProgressIterator};
use rayon::iter::IntoParallelIterator;
use serde::{Deserialize, Deserializer};
use time::format_description::well_known;
use rayon::iter::ParallelIterator;
use rayon::iter::ParallelBridge;

static ACCEPTED_IMAGE_EXTENSIONS: &[&str] = &["ARW", "HEIC", "JPG", "HEIF"];

#[derive(serde::Deserialize, Debug)]
struct ExifRaw {
	#[serde(rename = "CreateDate")]
	create_date: String,
	#[serde(rename = "SubSecTimeOriginal", deserialize_with = "deserialize_string_or_int")]
	subsec: u16,
}

#[derive(Debug)]
struct BurstFile {
	path: PathBuf,
	created: PlainDateTime,
}

pub fn scan_for_sequence(path: PathBuf) {
	let datefmt = format_description!("[year]:[month]:[day] [hour]:[minute]:[second]");

	let valid_files = path.read_dir().unwrap()
		.filter_map(|e|e.ok())
		.filter(
			|e|
				e.file_type().unwrap().is_file() &&
					ACCEPTED_IMAGE_EXTENSIONS.contains(&e.path().extension().unwrap_or_default().to_string_lossy().to_ascii_uppercase().as_str())
		).collect::<Vec<_>>();

	let len = valid_files.len();
	let all_files = valid_files.into_iter().progress_count(len as _).par_bridge().map(|valid_file| {
		let mut head = File::open(&valid_file.path()).unwrap();
		let mut buf = vec![0u8; 2usize.pow(16)];
		head.read_exact(buf.as_mut_slice()).unwrap();

		let mut exiftool = Command::new("exiftool").args(["-json", "-"])
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.spawn().unwrap();
		let stdin = exiftool.stdin.as_mut().unwrap();
		stdin.write_all(&buf).unwrap();

		let res = exiftool.wait_with_output().unwrap();
		// eprintln!("{}", String::from_utf8(res.stdout.clone()).unwrap());
		// fs::write("out.json", res.stdout.clone()).unwrap();
		let ser: Vec<ExifRaw> = serde_json::from_slice(&res.stdout).unwrap();
		let ser = &ser[0];
		BurstFile {
			path: valid_file.path(),
			created: PlainDateTime::parse(&ser.create_date, &datefmt).unwrap().replace_microsecond(ser.subsec as u32 * 1000).unwrap(),
		}
	}).collect::<Vec<_>>();
	dbg!(all_files);
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