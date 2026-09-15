use std::path::PathBuf;

static ACCEPTED_IMAGE_EXTENSIONS: &[&str] = &["ARW", "HEIC", "JPG", "HEIF"];

pub fn scan_for_sequence(path: PathBuf) {
	let valid_files = path.read_dir().unwrap()
		.filter_map(|e|e.ok())
		.filter(
			|e|
				e.file_type().unwrap().is_dir() &&
					ACCEPTED_IMAGE_EXTENSIONS.contains(dbg!(&e.path().extension().unwrap_or_default().to_string_lossy().to_ascii_uppercase().as_str()))
		).collect::<Vec<_>>();
	for p in valid_files {
		dbg!(p);
	}
}