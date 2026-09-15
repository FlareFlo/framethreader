use std::path::PathBuf;

pub fn scan_for_sequence(path: PathBuf) {
	for p in path.read_dir().unwrap() {
		dbg!(p);
	}
}