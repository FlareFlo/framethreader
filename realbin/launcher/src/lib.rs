#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod sequence_detection;

pub const RUNMODE: &str = "launcher";

use std::path::Path;
use std::path::PathBuf;
use std::thread;
use eframe::egui;
use crate::sequence_detection::scan_for_sequence;

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

enum LauncherState {
	Initial,
	BasedirPicked {
		path: PathBuf
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
		match self.state {
			LauncherState::Initial =>self.initial_picker(ui, frame),
			LauncherState::BasedirPicked {ref path} => {self.scan_basedir(ui, frame, path.clone())},
		}
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
					self.state = LauncherState::BasedirPicked {path};
				}
			}
		});
	}

	pub fn scan_basedir(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame, basedir: PathBuf) {
		egui::CentralPanel::default().show(ui, |ui| {
			ui.heading("Framethreader");
			ui.label("Helo");
			let bd = basedir.clone();
			thread::spawn(|| scan_for_sequence(bd));
		});
	}
}