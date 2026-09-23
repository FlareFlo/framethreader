#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod burst;
mod sequence_detection;

pub const RUNMODE: &str = "launcher";

use std::sync::Arc;
use crate::sequence_detection::{detect_by_time, scan_images};
use burst::BurstFile;
use eframe::egui;
use egui::ProgressBar;
use std::ops::Not;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::{mem, thread};
use std::collections::HashMap;
use std::sync::mpsc;
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
    Loaded(egui::TextureHandle),
}

struct MyApp {
    state: LauncherState,
    images: HashMap<(PathBuf, EmbeddedImageType), ThumbnailState>,
    image_rx: mpsc::Receiver<(
        PathBuf,
        EmbeddedImageType,
        Option<egui::ColorImage>,
    )>,
    image_tx: mpsc::Sender<(
        PathBuf,
        EmbeddedImageType,
        Option<egui::ColorImage>,
    )>,
    thread_limit: usize,
    active_preview: Option<usize>,
    selected_folder: Option<PathBuf>,
    exiftool_version: Option<String>,
    ffmpeg_version: Option<String>,
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

fn get_command_version(cmd: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(cmd).args(args).output().ok()?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Some(stdout.lines().next().unwrap_or("").to_string())
    } else {
        None
    }
}

impl Default for MyApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            state: LauncherState::Initial,
            images: Default::default(),
            image_rx: rx,
            image_tx: tx,
            thread_limit: thread::available_parallelism().unwrap().get(),
            active_preview: None,
            selected_folder: None,
            exiftool_version: get_command_version("exiftool", &["-ver"]),
            ffmpeg_version: get_command_version("ffmpeg", &["-version"]),
        }
    }
}

impl eframe::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Poll for thumbnails and previews
        while let Ok((path, img_type, color_image_opt)) = self.image_rx.try_recv() {
            let key = (path, img_type);
            if let Some(color_image) = color_image_opt {
                let name = match img_type {
                    EmbeddedImageType::Preview => "preview",
                    EmbeddedImageType::Thumbnail => "thumbnail",
                    EmbeddedImageType::Full => "full",
                };
                let texture =
                    ui.ctx()
                        .load_texture(name, color_image, egui::TextureOptions::LINEAR);
                self.images.insert(key, ThumbnailState::Loaded(texture));
            } else {
                self.images.remove(&key);
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
            LauncherState::Initial => {
                Self::initial_picker(
                    ui, 
                    frame, 
                    &mut self.thread_limit, 
                    &mut self.selected_folder,
                    &self.exiftool_version,
                    &self.ffmpeg_version
                )
            }
            LauncherState::Scanning { .. } => {
                Self::scan_basedir(ui, frame);
                None
            }
            LauncherState::CompletedScan { files, min_frames } => {
                Self::show_scan_results(
                    ui,
                    frame,
                    files,
                    min_frames,
                    &mut self.images,
                    &self.image_tx,
                    &mut self.active_preview,
                );
                None
            }
        };

        if let Some(state) = new_state {
            self.state = state;
        }
    }
}

impl MyApp {
    pub fn initial_picker(
        ui: &mut egui::Ui,
        _frame: &mut eframe::Frame,
        thread_limit: &mut usize,
        selected_folder: &mut Option<PathBuf>,
        exiftool_version: &Option<String>,
        ffmpeg_version: &Option<String>,
    ) -> Option<LauncherState> {
        let mut result = None;

        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(egui::RichText::new("Framethreader").strong().size(40.0));
                ui.add_space(30.0);

                ui.allocate_ui_with_layout(
                    egui::vec2(450.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        // Dependencies Card
                        egui::Frame::group(ui.style())
                            .fill(ui.visuals().window_fill())
                            .inner_margin(16.0)
                            .show(ui, |ui| {
                                ui.set_min_width(420.0);
                                ui.label(egui::RichText::new("Dependencies").strong().size(18.0));
                                ui.add_space(10.0);

                                ui.horizontal(|ui| {
                                    if let Some(ver) = exiftool_version {
                                        ui.label(egui::RichText::new("exiftool").color(egui::Color32::GREEN));
                                        ui.label(ver);
                                    } else {
                                        ui.label(egui::RichText::new("❌ exiftool").color(egui::Color32::RED));
                                        ui.label("Not found (Optional for now)");
                                    }
                                });
                                ui.horizontal(|ui| {
                                    if let Some(ver) = ffmpeg_version {
                                        ui.label(egui::RichText::new("ffmpeg").color(egui::Color32::GREEN));
                                        ui.label(ver);
                                    } else {
                                        ui.label(egui::RichText::new("❌ ffmpeg").color(egui::Color32::RED));
                                        ui.label("Not found");
                                    }
                                });
                            });

                        ui.add_space(20.0);

                        // Settings Card
                        egui::Frame::group(ui.style())
                            .fill(ui.visuals().window_fill())
                            .inner_margin(16.0)
                            .show(ui, |ui| {
                                ui.set_min_width(420.0);
                                ui.label(egui::RichText::new("Settings").strong().size(18.0));
                                ui.add_space(10.0);

                                ui.add(
                                    egui::Slider::new(
                                        thread_limit,
                                        1..=thread::available_parallelism().unwrap().get(),
                                    )
                                    .text("Global Thread Limit"),
                                );
                            });

                        ui.add_space(40.0);

                        // Action Area
                        ui.vertical_centered(|ui| {
                            if ui.add_sized([220.0, 40.0], egui::Button::new(egui::RichText::new("📁 Select Folder").size(16.0))).clicked() {
                                let picked = rfd::FileDialog::new().pick_folder();
                                if let Some(path) = picked {
                                    *selected_folder = Some(path);
                                }
                            }

                            ui.add_space(10.0);

                            if let Some(path) = selected_folder {
                                ui.label(egui::RichText::new(format!("Selected: {}", path.display())).weak());
                                ui.add_space(20.0);

                                let scan_btn = egui::Button::new(egui::RichText::new("Start Scan").strong().size(24.0))
                                    .fill(ui.visuals().selection.bg_fill);
                                if ui.add_sized([300.0, 60.0], scan_btn).clicked() {
                                    let _ = rayon::ThreadPoolBuilder::new()
                                        .num_threads(*thread_limit)
                                        .build_global();

                                    let bd = path.clone();
                                    let handle = thread::spawn(|| {
                                        let images = scan_images(bd);
                                        let frames = detect_by_time(images, Duration::milliseconds(300));
                                        frames
                                    });
                                    result = Some(LauncherState::Scanning { path: path.clone(), handle });
                                }
                            } else {
                                ui.label(egui::RichText::new("No folder selected").weak());
                            }
                        });
                    },
                );
            });
        });

        result
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
        images: &mut HashMap<(PathBuf, EmbeddedImageType), ThumbnailState>,
        image_tx: &std::sync::mpsc::Sender<(
            PathBuf,
            EmbeddedImageType,
            Option<egui::ColorImage>,
        )>,
        active_preview: &mut Option<usize>,
    ) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Scan Results");
            ui.add(egui::Slider::new(min_frames, 1..=100).text("Min Frames"));
            egui_extras::TableBuilder::new(ui)
                .striped(true)
                .column(egui_extras::Column::initial(160.0).at_least(160.0))
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
                    header.col(|ui| {
                        ui.heading("Actions");
                    });
                })
                .body(|mut body| {
                    for (i, group) in files.iter_mut().enumerate() {
                        if group.len() < *min_frames {
                            continue;
                        }
                        body.row(120.0, |mut row| {
                            row.col(|ui| {
                                let mut all_loaded = true;
                                for frame in group.iter() {
                                    let frame_path = frame.path();
                                    let key = (frame_path.clone(), EmbeddedImageType::Thumbnail);
                                    if !images.contains_key(&key) {
                                        images.insert(key.clone(), ThumbnailState::Loading);
                                        all_loaded = false;

                                        let tx = image_tx.clone();
                                        let path_clone = frame_path.clone();
                                        let ctx = ui.ctx().clone();
                                        rayon::spawn(move || {
                                            use EmbeddedImageType;
                                            let rgb_opt = BurstFile::extract_embedded_image(
                                                &path_clone,
                                                EmbeddedImageType::Thumbnail,
                                            );
                                            let color_image = rgb_opt.map(|img| {
                                                let size = [img.width() as _, img.height() as _];
                                                let pixels = img.as_flat_samples();
                                                egui::ColorImage::from_rgb(size, pixels.as_slice())
                                            });
                                            let _ = tx.send((
                                                path_clone,
                                                EmbeddedImageType::Thumbnail,
                                                color_image,
                                            ));
                                            ctx.request_repaint();
                                        });
                                    } else if let Some(ThumbnailState::Loading) = images.get(&key) {
                                        all_loaded = false;
                                    }
                                }

                                if all_loaded {
                                    let time = ui.input(|i| i.time);
                                    let play_idx = (time * 10.0) as usize % group.len();
                                    let path = group[play_idx].path().clone();
                                    if let Some(ThumbnailState::Loaded(texture)) =
                                        images.get(&(path, EmbeddedImageType::Thumbnail))
                                    {
                                        ui.add(
                                            egui::Image::new(texture)
                                                .fit_to_exact_size(egui::vec2(160.0, 120.0)),
                                        );
                                    }
                                } else {
                                    render_fallback_spinner(
                                        ui,
                                        group,
                                        images,
                                        egui::vec2(160.0, 120.0),
                                        egui::vec2(20.0, 20.0),
                                    );
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
                        let mut all_loaded = true;
                        for frame in group.iter() {
                            let frame_path = frame.path();
                            let key = (frame_path.clone(), EmbeddedImageType::Preview);
                            if !images.contains_key(&key) {
                                images.insert(key.clone(), ThumbnailState::Loading);
                                all_loaded = false;

                                let tx = image_tx.clone();
                                let path_clone = frame_path.clone();
                                let ctx = ui.ctx().clone();
                                rayon::spawn(move || {
                                    use EmbeddedImageType;
                                    let rgb_opt = BurstFile::extract_embedded_image(
                                        &path_clone,
                                        EmbeddedImageType::Preview,
                                    );
                                    let color_image = rgb_opt.map(|img| {
                                        let size = [img.width() as _, img.height() as _];
                                        let pixels = img.as_flat_samples();
                                        egui::ColorImage::from_rgb(size, pixels.as_slice())
                                    });
                                    let _ = tx.send((
                                        path_clone,
                                        EmbeddedImageType::Preview,
                                        color_image,
                                    ));
                                    ctx.request_repaint();
                                });
                            } else if let Some(ThumbnailState::Loading) = images.get(&key) {
                                all_loaded = false;
                            }
                        }

                        if all_loaded {
                            let time = ui.input(|i| i.time);
                            let play_idx = (time * 10.0) as usize % group.len();
                            let play_path = group[play_idx].path().clone();
                            ui.heading(format!(
                                "Playing {} frames... ({} / {})",
                                group.len(),
                                play_idx + 1,
                                group.len()
                            ));

                            if let Some(ThumbnailState::Loaded(texture)) =
                                images.get(&(play_path, EmbeddedImageType::Preview))
                            {
                                ui.add(
                                    egui::Image::new(texture)
                                        .fit_to_exact_size(egui::vec2(800.0, 600.0)),
                                );
                            }
                        } else {
                            ui.heading(format!("Loading {} frames...", group.len()));
                            render_fallback_spinner(
                                ui,
                                group,
                                images,
                                egui::vec2(800.0, 600.0),
                                egui::vec2(30.0, 30.0),
                            );
                        }
                    });
            }
        }
        if !preview_open {
            *active_preview = None;
        }
    }
}

fn render_fallback_spinner(
    ui: &mut egui::Ui,
    group: &[BurstFile],
    images: &HashMap<(PathBuf, EmbeddedImageType), ThumbnailState>,
    size: egui::Vec2,
    spinner_size: egui::Vec2,
) {
    let mut fallback = None;
    let mid_path = group[group.len() / 2].path();

    if let Some(ThumbnailState::Loaded(tex)) =
        images.get(&(mid_path.clone(), EmbeddedImageType::Preview))
    {
        fallback = Some(tex);
    } else {
        for frame in group.iter() {
            if let Some(ThumbnailState::Loaded(tex)) =
                images.get(&(frame.path().clone(), EmbeddedImageType::Preview))
            {
                fallback = Some(tex);
                break;
            }
        }
    }

    if fallback.is_none() {
        if let Some(ThumbnailState::Loaded(tex)) =
            images.get(&(mid_path.clone(), EmbeddedImageType::Thumbnail))
        {
            fallback = Some(tex);
        } else {
            for frame in group.iter() {
                if let Some(ThumbnailState::Loaded(tex)) =
                    images.get(&(frame.path().clone(), EmbeddedImageType::Thumbnail))
                {
                    fallback = Some(tex);
                    break;
                }
            }
        }
    }

    if let Some(tex) = fallback {
        let response = ui.add(
            egui::Image::new(tex)
                .fit_to_exact_size(size)
                .tint(egui::Color32::from_gray(100)),
        );
        let center = response.rect.center();
        let spinner_rect = egui::Rect::from_center_size(center, spinner_size);
        ui.put(spinner_rect, egui::Spinner::new());
    } else {
        let (rect, _resp) = ui.allocate_exact_size(size, egui::Sense::hover());
        let center = rect.center();
        let spinner_rect = egui::Rect::from_center_size(center, spinner_size);
        ui.put(spinner_rect, egui::Spinner::new());
    }
}
