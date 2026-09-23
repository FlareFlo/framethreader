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
use crate::burst::EmbeddedImageType;

pub fn realmain() {
	env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 768.0]),
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

pub enum ThumbnailState {
    Loading,
    Loaded(eframe::egui::TextureHandle),
}

struct MyApp {
    state: LauncherState,
    thumbnails: std::collections::HashMap<PathBuf, ThumbnailState>,
    previews: std::collections::HashMap<PathBuf, ThumbnailState>,
    image_tx: std::sync::mpsc::Sender<(PathBuf, bool, Option<eframe::egui::ColorImage>)>,
    image_rx: std::sync::mpsc::Receiver<(PathBuf, bool, Option<eframe::egui::ColorImage>)>,
    active_preview: Option<usize>,
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
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            state: LauncherState::Initial,
            thumbnails: Default::default(),
            previews: Default::default(),
            image_tx: tx,
            image_rx: rx,
            active_preview: None,
        }
    }
}

impl eframe::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Poll for thumbnails and previews
        while let Ok((path, is_preview, color_image_opt)) = self.image_rx.try_recv() {
            if let Some(color_image) = color_image_opt {
                let texture = ui.ctx().load_texture(
                    if is_preview { "preview" } else { "thumbnail" }, 
                    color_image, 
                    egui::TextureOptions::LINEAR
                );
                if is_preview {
                    self.previews.insert(path, ThumbnailState::Loaded(texture));
                } else {
                    self.thumbnails.insert(path, ThumbnailState::Loaded(texture));
                }
            } else {
                if is_preview {
                    self.previews.remove(&path);
                } else {
                    self.thumbnails.remove(&path);
                }
            }
        }

        // Scanning -> CompletedScanning
        if let LauncherState::Scanning { handle, .. } = &self.state {
            if handle.is_finished() {
                if let LauncherState::Scanning { handle, .. } = mem::take(&mut self.state) {
                    let res = handle.join().unwrap();
                    self.state = LauncherState::CompletedScan {
                        files: res,
                        min_frames: 10,
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
                Self::show_scan_results(ui, frame, files, min_frames, &mut self.thumbnails, &mut self.previews, &self.image_tx, &mut self.active_preview);
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
        thumbnails: &mut std::collections::HashMap<PathBuf, ThumbnailState>,
        previews: &mut std::collections::HashMap<PathBuf, ThumbnailState>,
        image_tx: &std::sync::mpsc::Sender<(PathBuf, bool, Option<eframe::egui::ColorImage>)>,
        active_preview: &mut Option<usize>,
    ) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Scan Results");
            ui.add(Slider::new(min_frames, 1..=100).text("Min Frames"));
            egui_extras::TableBuilder::new(ui)
                .striped(true)
                .column(egui_extras::Column::initial(160.0).at_least(160.0))
                .column(egui_extras::Column::initial(100.0).at_least(80.0))
                .column(egui_extras::Column::initial(100.0).at_least(80.0))
                .column(egui_extras::Column::remainder())
                .header(20.0, |mut header| {
                    header.col(|ui| { ui.heading("Thumbnail"); });
                    header.col(|ui| { ui.heading("Length"); });
                    header.col(|ui| { ui.heading("Duration"); });
                    header.col(|ui| { ui.heading("Actions"); });
                })
                .body(|mut body| {
                    for (i, group) in files.iter_mut().enumerate() {
                        if group.len() < *min_frames {
                            continue;
                        }
                        body.row(120.0, |mut row| {
                            row.col(|ui| {
                                let time = ui.input(|i| i.time);
                                let play_idx = (time * 10.0) as usize % group.len();
                                let path = group[play_idx].path().clone();
                                
                                if !thumbnails.contains_key(&path) {
                                    thumbnails.insert(path.clone(), ThumbnailState::Loading);
                                    let tx = image_tx.clone();
                                    let path_clone = path.clone();
                                    let ctx = ui.ctx().clone();
                                    std::thread::spawn(move || {
                                        use crate::burst::EmbeddedImageType;
                                        let rgb_opt = BurstFile::extract_embedded_image(&path_clone, EmbeddedImageType::Thumbnail);
                                        let color_image = rgb_opt.map(|img| {
                                            let size = [img.width() as _, img.height() as _];
                                            let pixels = img.as_flat_samples();
                                            egui::ColorImage::from_rgb(size, pixels.as_slice())
                                        });
                                        let _ = tx.send((path_clone, false, color_image));
                                        ctx.request_repaint();
                                    });
                                }
                                
                                match thumbnails.get(&path) {
                                    Some(ThumbnailState::Loaded(texture)) => {
                                        ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(160.0, 120.0)));
                                    }
                                    _ => {
                                        ui.spinner();
                                    }
                                }
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
                            row.col(|ui| {
                                if ui.button("Preview Sequence").clicked() {
                                    *active_preview = Some(i);
                                }
                            });
                        });
                    }
                });
        });

        ui.ctx().request_repaint(); // always animate the table

        let mut preview_open = active_preview.is_some();
        if let Some(idx) = *active_preview {
            if let Some(group) = files.get(idx) {
                egui::Window::new(format!("Sequence Preview: Group {}", idx + 1))
                    .open(&mut preview_open)
                    .default_size(egui::vec2(800.0, 600.0))
                    .show(ui.ctx(), |ui| {
                        let time = ui.input(|i| i.time);
                        let play_idx = (time * 10.0) as usize % group.len();
                        let play_path = group[play_idx].path().clone();

                        if !previews.contains_key(&play_path) {
                            previews.insert(play_path.clone(), ThumbnailState::Loading);
                            let tx = image_tx.clone();
                            let path_clone = play_path.clone();
                            let ctx = ui.ctx().clone();
                            std::thread::spawn(move || {
                                use crate::burst::EmbeddedImageType;
                                let rgb_opt = BurstFile::extract_embedded_image(&path_clone, EmbeddedImageType::Preview);
                                let color_image = rgb_opt.map(|img| {
                                    let size = [img.width() as _, img.height() as _];
                                    let pixels = img.as_flat_samples();
                                    egui::ColorImage::from_rgb(size, pixels.as_slice())
                                });
                                let _ = tx.send((path_clone, true, color_image));
                                ctx.request_repaint();
                            });
                        }

                        ui.heading(format!("Playing {} frames... ({} / {})", group.len(), play_idx + 1, group.len()));
                        
                        match previews.get(&play_path) {
                            Some(ThumbnailState::Loaded(texture)) => {
                                ui.add(egui::Image::new(texture));
                            }
                            _ => {
                                if let Some(ThumbnailState::Loaded(mid_tex)) = thumbnails.get(&play_path) {
                                    ui.add(egui::Image::new(mid_tex).fit_to_exact_size(egui::vec2(800.0, 600.0)));
                                } else {
                                    ui.spinner();
                                }
                            }
                        }
                    });
            }
        }
        if !preview_open {
            *active_preview = None;
        }
    }
}
