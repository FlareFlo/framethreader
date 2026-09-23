#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod sequence_detection;
mod burst;

pub const RUNMODE: &str = "launcher";

use crate::sequence_detection::{detect_by_time, scan_images};
use burst::BurstFile;
use eframe::egui;
use egui::{ProgressBar, Slider};
use std::ops::Not;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::{mem, thread};
use time::Duration;

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
    )
    .unwrap();
}

struct MyApp {
    state: LauncherState,
    thumbnails: std::collections::HashMap<PathBuf, eframe::egui::TextureHandle>,
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
        min_frames: usize,
    },
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            state: LauncherState::Initial,
            thumbnails: Default::default(),
        }
    }
}

impl eframe::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Scanning -> CompletedScanning
        if let LauncherState::Scanning { handle, .. } = &self.state {
            if handle.is_finished() {
                if let LauncherState::Scanning { handle, .. } = mem::take(&mut self.state) {
                    let res = handle.join().unwrap();
                    self.state = LauncherState::CompletedScan {
                        files: res,
                        min_frames: 3,
                    };
                }
            }
        }

        // Only render UI here
        let new_state = match &mut self.state {
            LauncherState::Initial => Self::initial_picker(ui, frame),
            LauncherState::Scanning { .. } => {
                Self::scan_basedir(ui, frame);
                None
            }
            LauncherState::CompletedScan { files, min_frames } => {
                Self::show_scan_results(ui, frame, files, min_frames, &mut self.thumbnails);
                None
            }
        };

        if let Some(state) = new_state {
            self.state = state;
        }
    }
}

impl MyApp {
    pub fn initial_picker(ui: &mut egui::Ui, _frame: &mut eframe::Frame) -> Option<LauncherState> {
        egui::CentralPanel::default()
            .show(ui, |ui| {
                ui.heading("Framethreader");

                if ui.button("Select folder").clicked() {
                    let picked = rfd::FileDialog::new().pick_folder();
                    if let Some(path) = picked {
                        let bd = path.clone();
                        let handle = thread::spawn(|| {
                            let images = scan_images(bd);
                            let frames = detect_by_time(images, Duration::milliseconds(300));
                            frames
                        });
                        return Some(LauncherState::Scanning { path, handle });
                    }
                }
                None
            })
            .inner
    }

    pub fn scan_basedir(ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Framethreader");
            let progress = sequence_detection::get_progress();
            let msg = format!(
                "{} {}",
                progress.current_task,
                progress
                    .current_file
                    .rsplit_once("/")
                    .map(|e| e.1)
                    .unwrap_or(&progress.current_file)
            );
            ui.add(
                ProgressBar::new(progress.progress_ratio())
                    .show_percentage()
                    .text(msg)
                    .desired_width(300.0),
            );
            if progress.complete().not() {
                ui.request_repaint();
            }
        });
    }

    pub fn show_scan_results(
        ui: &mut egui::Ui,
        _frame: &mut eframe::Frame,
        files: &mut [Vec<BurstFile>],
        min_frames: &mut usize,
        thumbnails: &mut std::collections::HashMap<PathBuf, eframe::egui::TextureHandle>,
    ) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Scan Results");
            ui.add(Slider::new(min_frames, 1..=100).text("Min Frames"));
            egui_extras::TableBuilder::new(ui)
                .striped(true)
                .column(egui_extras::Column::initial(100.0).at_least(80.0))
                .column(egui_extras::Column::initial(100.0).at_least(80.0))
                .column(egui_extras::Column::remainder())
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.heading("Thumbnail");
                    });
                    header.col(|ui| {
                        ui.heading("Length");
                    });
                    header.col(|ui| {
                        ui.heading("Duration");
                    });
                })
                .body(|mut body| {
                    for group in files.iter_mut() {
                        if group.len() < *min_frames {
                            continue;
                        }
                        body.row(80.0, |mut row| {
                            row.col(|ui| {
                                let mid_idx = group.len() / 2;
                                let mid_frame = &mut group[mid_idx];
                                let path = mid_frame.path().clone();
                                
                                let texture = thumbnails.entry(path).or_insert_with(|| {
                                    if mid_frame.thumbnail().is_none() {
                                        mid_frame.gen_thumbnail();
                                    }
                                    let img = mid_frame.thumbnail().as_ref().unwrap();
                                    let size = [img.width() as _, img.height() as _];
                                    let pixels = img.as_flat_samples();
                                    let color_image = egui::ColorImage::from_rgb(size, pixels.as_slice());
                                    ui.ctx().load_texture("thumbnail", color_image, egui::TextureOptions::LINEAR)
                                });
                                
                                ui.add(egui::Image::new(&*texture).fit_to_exact_size(egui::vec2(80.0, 80.0)));
                            });
                            row.col(|ui| {
                                ui.label(format!("{} frames", group.len()));
                            });
                            row.col(|ui| {
                                if let (Some(first), Some(last)) = (group.first(), group.last()) {
                                    let duration = *last.created() - *first.created();
                                    ui.label(format!("{:.2}s", duration.as_seconds_f32()));
                                } else {
                                    ui.label("-");
                                }
                            });
                        });
                    }
                });
        });
    }
}
