#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod sequence_detection;

pub const RUNMODE: &str = "launcher";

use std::ops::Not;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::{mem, thread};
use std::thread::JoinHandle;
use eframe::egui;
use egui::ProgressBar;
use time::Duration;
use crate::sequence_detection::{detect_by_time, scan_images, BurstFile};

pub fn realmain() {
	env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
		..Default::default()
	};
	eframe::run_native(
		"Framethreader",
		options,
		Box::new(|cc| {
			// This gives us image support:
			egui_extras::install_image_loaders(&cc.egui_ctx);

			Ok(Box::<MyApp>::default())
		}),
	).unwrap();
}

struct MyApp {
	state: LauncherState,
}

#[derive(Default)]

enum LauncherState {
	#[default]
	Initial,
	Scanning {
		path: PathBuf,
		handle: JoinHandle<Vec<Vec<BurstFile>>>,
	},
	CompletedScan {
		files: Vec<Vec<BurstFile>>,
	}
}

impl Default for MyApp {
	fn default() -> Self {
		Self {
			state: LauncherState::Initial,
		}
	}
}

impl eframe::App for MyApp {
	fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
		let state= mem::take(&mut self.state);
		self.state = match state {
			LauncherState::Initial =>{self.initial_picker(ui, frame); state},
			LauncherState::Scanning { handle, path} => {
				Self::scan_basedir(ui, frame);
				if handle.is_finished() {
					let res = handle.join().unwrap();
					LauncherState::CompletedScan {files: res}
				} else {
					state
				}
			},
			LauncherState::CompletedScan {ref files} => {Self::show_scan_results(ui, frame, files); state}
		};
	}
}

impl MyApp {
	pub fn initial_picker(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
		egui::CentralPanel::default().show(ui, |ui| {
			ui.heading("Framethreader");

			if ui.button("Select folder").clicked() {
				let picked = rfd::FileDialog::new()
					.pick_folder();
				if let Some(path) = picked {
					let bd = path.clone();
					let handle = thread::spawn(|| {
						let images = scan_images(bd);
						let frames = detect_by_time(images, Duration::milliseconds(300), 3);
						frames
					});
					self.state = LauncherState::Scanning {path, handle };
				}
			}
		});
	}

	pub fn scan_basedir(ui: &mut egui::Ui, _frame: &mut eframe::Frame)  {
		egui::CentralPanel::default().show(ui, |ui| {
			ui.heading("Framethreader");
			let progress = sequence_detection::get_progress();
			let msg = format!("{} {}", progress.current_task, progress.current_file.rsplit_once("/").map(|e|e.1).unwrap_or(&progress.current_file));
			ui.add(ProgressBar::new(progress.progress_ratio()).show_percentage().text(msg).desired_width(300.0));
			if progress.complete().not() {
				ui.request_repaint();
			} else {

			}
		});
	}

	pub fn show_scan_results(ui: &mut egui::Ui, _frame: &mut eframe::Frame, files: &Vec<Vec<BurstFile>>)  {

	}
}