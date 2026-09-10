use chrono::{Duration, NaiveDateTime};
use exif::{In, Reader, Tag};
use rayon::prelude::*;
use std::fs;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Debug)]
pub struct PhotoMeta {
	pub path: PathBuf,
	datetime: NaiveDateTime,
}

pub fn extract_timestamps(path: PathBuf) -> Option<PhotoMeta> {
	let file = fs::File::open(&path).ok()?;

	// 64KB buffer: Large enough to grab the EXIF header in one network request,
	// small enough to prevent downloading the actual RAW image data.
	let mut bufreader = BufReader::with_capacity(64 * 1024, file);
	let exifreader = Reader::new();

	let exif = exifreader.read_from_container(&mut bufreader).ok()?;

	let timestamp = exif.get_field(Tag::DateTimeOriginal, In::PRIMARY)?
		.display_value().to_string().replace("\"", "").trim().to_string();

	let sub_second = exif.get_field(Tag::SubSecTimeOriginal, In::PRIMARY)?
		.display_value().to_string().replace("\"", "").trim().to_string();

	let datetime_str = format!("{}.{}", timestamp, sub_second);

	// Use %f for the fractional parser (it automatically handles the millisecond digits)
	let datetime = match NaiveDateTime::parse_from_str(&datetime_str, "%Y-%m-%d %H:%M:%S.%f") {
		Ok(dt) => dt,
		Err(e) => {
			eprintln!("Failed to parse '{}': {}", datetime_str, e);
			return None;
		}
	};

	let meta = PhotoMeta { path, datetime };
	Some(meta)
}

pub fn group_into_bursts(mut metadata: Vec<PhotoMeta>) -> Vec<Vec<PhotoMeta>> {
	// Ensure chronological order
	metadata.sort_by_key(|meta| meta.datetime);

	let mut bursts: Vec<Vec<PhotoMeta>> = Vec::new();
	let mut current_burst: Vec<PhotoMeta> = Vec::new();

	// 5 fps = 200ms interval. We use 250ms to allow for slight camera timing jitter.
	let threshold = Duration::milliseconds(250);

	for meta in metadata {
		if let Some(last_photo) = current_burst.last() {
			// Calculate time difference between this photo and the last one
			let duration_since_last = meta.datetime - last_photo.datetime;

			if duration_since_last <= threshold {
				// It belongs to the current burst
				current_burst.push(meta);
			} else {
				// Gap is too large! Save the current burst and start a new one
				bursts.push(current_burst);
				current_burst = vec![meta];
			}
		} else {
			current_burst.push(meta);
		}
	}

	// Push the final remaining burst
	if !current_burst.is_empty() {
		bursts.push(current_burst);
	}

	// Filter out isolated photos (e.g., bursts must have at least 2 or 3 photos)
	bursts.into_iter().filter(|b| b.len() > 2).collect()
}