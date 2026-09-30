use std::cmp::PartialEq;
use common::burst::BurstFile;
use common::sequence_detection::{self, detect_by_time, scan_images};
use eframe::egui;
use std::path::PathBuf;
use std::thread;
use time::Duration;

pub const RUNMODE: &str = "threader";

pub fn realmain() {
    let path = std::env::args().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Threader",
        options,
        Box::new(|_cc| Ok(Box::new(ThreaderApp::new(path)))),
    )
    .unwrap();
}

enum ThreaderState {
    Initial(Option<PathBuf>),
    Scanning {
        folder: PathBuf,
        handle: thread::JoinHandle<Vec<BurstFile>>,
    },
    ReadyToProcess {
        folder: PathBuf,
        burst: Vec<BurstFile>,
    },
    Rendering {
        folder: PathBuf,
        handle: thread::JoinHandle<bool>,
        progress_rx: std::sync::mpsc::Receiver<(usize, String)>,
        current_progress: String,
        current_frame: usize,
        total_frames: usize,
    },
    Done {
        folder: PathBuf,
        success: bool,
    },
    Error(String),
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum Codec {
    H264,
    HEVC,
    ProRes,
    DNxHR,
    AV1,
    FcpXml,
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum Timing {
    TrueExifVfr,
    TrueExifPeak,
    TrueExifHighPrecision,
    FixedFpsAverage,
    CustomFps,
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum Preset {
    Custom,
    WebPortable,
    HighEfficiency,
    Editing,
}

impl Preset {
    pub(crate) fn to_settings(&self) -> RenderSettings {
        match self {
            Preset::WebPortable => RenderSettings {
                preset: *self,
                codec: Codec::H264,
                timing: Timing::TrueExifVfr,
                custom_fps: 120.0,
                high_precision_fps: 120,
                all_intra: false,
            },
            Preset::HighEfficiency => RenderSettings {
                preset: *self,
                codec: Codec::AV1,
                timing: Timing::TrueExifVfr,
                custom_fps: 120.0,
                high_precision_fps: 120,
                all_intra: false,
            },
            Preset::Editing => RenderSettings {
                preset: *self,
                codec: Codec::ProRes,
                timing: Timing::TrueExifPeak,
                custom_fps: 120.0,
                high_precision_fps: 120,
                all_intra: false,
            },
            Preset::Custom => RenderSettings {
                preset: *self,
                codec: Codec::H264,
                timing: Timing::TrueExifPeak,
                custom_fps: 120.0,
                high_precision_fps: 120,
                all_intra: false,
            },
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
struct RenderSettings {
    preset: Preset,
    codec: Codec,
    timing: Timing,
    custom_fps: f32,
    high_precision_fps: u32,
    all_intra: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Preset::WebPortable.to_settings()
    }
}

struct ThreaderApp {
    state: ThreaderState,
    settings: RenderSettings,
}

impl ThreaderApp {
    fn new(folder: Option<PathBuf>) -> Self {
        Self {
            state: ThreaderState::Initial(folder),
            settings: RenderSettings::default(),
        }
    }
}

impl eframe::App for ThreaderApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Handle scanning completion
        if let ThreaderState::Scanning { handle, .. } = &self.state {
            if handle.is_finished() {
                if let ThreaderState::Scanning { folder, handle } = std::mem::replace(&mut self.state, ThreaderState::Error(String::new())) {
                    let files = handle.join().unwrap();
                    let bursts = detect_by_time(files, Duration::seconds(2)); // Use a 2-second threshold for burst detection
                    
                    if bursts.is_empty() {
                        self.state = ThreaderState::Error("No images found in the selected folder.".to_string());
                    } else if bursts.len() > 1 {
                        self.state = ThreaderState::Error(format!("Found {} separate bursts in this folder! Threader expects exactly 1 burst per folder.", bursts.len()));
                    } else {
                        self.state = ThreaderState::ReadyToProcess {
                            folder,
                            burst: bursts.into_iter().next().unwrap(),
                        };
                    }
                }
            }
        }
        
        if let ThreaderState::Rendering { handle, .. } = &self.state {
            if handle.is_finished() {
                if let ThreaderState::Rendering { folder, handle, .. } = std::mem::replace(&mut self.state, ThreaderState::Error(String::new())) {
                    let success = handle.join().unwrap_or(false);
                    self.state = ThreaderState::Done { folder, success };
                }
            }
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.heading("Threader - FFMPEG Burst Processor");
                ui.add_space(20.0);
    
                match &mut self.state {
                ThreaderState::Initial(folder_opt) => {
                    if let Some(folder) = folder_opt {
                        ui.label(format!("Selected Folder: {}", folder.display()));
                        ui.add_space(10.0);
                        if ui.button("Scan Folder for Burst").clicked() {
                            let path_clone = folder.clone();
                            let handle = thread::spawn(move || scan_images(path_clone));
                            self.state = ThreaderState::Scanning {
                                folder: folder.clone(),
                                handle,
                            };
                        }
                    } else {
                        ui.label("No folder selected.");
                        ui.add_space(10.0);
                        if ui.button("Select Folder").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                *folder_opt = Some(path);
                            }
                        }
                    }
                }
                ThreaderState::Scanning { .. } => {
                    let progress = sequence_detection::get_progress();
                    ui.spinner();
                    ui.label(&progress.current_task);
                    let ratio = if progress.total > 0 { progress.progress_ratio() } else { 0.0 };
                    ui.add(egui::ProgressBar::new(ratio).show_percentage());
                    ui.label(format!("{}/{} Files", progress.completed, progress.total));
                    ui.ctx().request_repaint();
                }
                ThreaderState::ReadyToProcess { folder, burst } => {
                    ui.label(format!("Ready to process burst from: {}", folder.display()));
                    ui.label(format!("Frames found: {}", burst.len()));
                    ui.add_space(10.0);
                    
                    let average_fps = if burst.len() > 1 {
                        let total_dur = (*burst.last().unwrap().created() - *burst.first().unwrap().created()).as_seconds_f32().max(0.01);
                        ((burst.len() - 1) as f32 / total_dur).round() as u32
                    } else {
                        30
                    };
                    
                    let peak_fps = if burst.len() > 1 {
                        let mut raw_durations = Vec::with_capacity(burst.len() - 1);
                        for i in 0..burst.len() - 1 {
                            let dur = (*burst[i + 1].created() - *burst[i].created()).as_seconds_f32().max(0.01);
                            raw_durations.push(dur);
                        }
                        raw_durations.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        let min_dur = raw_durations[0]; // Shortest duration = highest FPS
                        (1.0 / min_dur).ceil() as u32
                    } else {
                        30
                    };
                    
                    ui.group(|ui| {
                        ui.heading("Output Settings");
                        
                        egui::Grid::new("settings_grid").num_columns(2).spacing([15.0, 10.0]).show(ui, |ui| {
                            ui.label("Preset:");
                            let mut selected = self.settings.preset;
                            egui::ComboBox::from_id_salt("preset_combo")
                                .selected_text(match selected {
                                    Preset::Custom => "Custom",
                                    Preset::WebPortable => "Web / Portable (H.264 MP4)",
                                    Preset::HighEfficiency => "High Efficiency (AV1 MP4)",
                                    Preset::Editing => "Editing (ProRes MOV)",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut selected, Preset::Custom, "Custom");
                                    ui.selectable_value(&mut selected, Preset::WebPortable, "Web / Portable (H.264 MP4)");
                                    ui.selectable_value(&mut selected, Preset::HighEfficiency, "High Efficiency (AV1 MP4)");
                                    ui.selectable_value(&mut selected, Preset::Editing, "Editing (ProRes MOV)");
                                });
                                
                            if selected != self.settings.preset {
                                if selected == Preset::Custom {
                                    self.settings.preset = Preset::Custom;
                                } else {
                                    self.settings = selected.to_settings();
                                }
                            }
                            ui.end_row();

                            let old_settings = self.settings;

                            ui.label("Codec:");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.settings.codec, Codec::H264, "H.264 (MP4)")
                                    .on_hover_text("Universally compatible and widely supported. Great for general sharing and web upload.");
                                ui.radio_value(&mut self.settings.codec, Codec::HEVC, "HEVC (MP4)")
                                    .on_hover_text("High Efficiency Video Coding. Yields much smaller file sizes than H.264, but requires newer hardware to play back.");
                                ui.radio_value(&mut self.settings.codec, Codec::AV1, "AV1 (MP4)")
                                    .on_hover_text("Next-generation open codec. Unbeatable file sizes and quality, but encoding is extremely slow.");
                                ui.radio_value(&mut self.settings.codec, Codec::ProRes, "ProRes (MOV)")
                                    .on_hover_text("Visually lossless, all-intra codec. Best for importing into video editors like Premiere or Resolve.");
                                ui.radio_value(&mut self.settings.codec, Codec::DNxHR, "DNxHR (MOV)")
                                    .on_hover_text("Avid's visually lossless, all-intra editing codec. Excellent performance on NLEs.");
                                ui.radio_value(&mut self.settings.codec, Codec::FcpXml, "FCPXML (Timeline)")
                                    .on_hover_text("Generates a DaVinci/Premiere timeline file");
                            });
                            ui.end_row();

                            if self.settings.codec == Codec::HEVC {
                                ui.label("");
                                ui.label(egui::RichText::new("Warning: HEVC playback on Windows often requires paid extensions!")
                                    .color(egui::Color32::from_rgb(255, 165, 0))
                                    .small());
                                ui.end_row();
                            }

                            // Prevent VFR for NLE codecs
                            if (self.settings.codec == Codec::ProRes || self.settings.codec == Codec::DNxHR) && self.settings.timing == Timing::TrueExifVfr {
                                self.settings.timing = Timing::TrueExifPeak;
                            }

                            ui.label("Timing:");
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    if self.settings.codec == Codec::ProRes || self.settings.codec == Codec::DNxHR {
                                        ui.add_enabled_ui(false, |ui| {
                                            ui.radio_value(&mut self.settings.timing, Timing::TrueExifVfr, "True EXIF (Native VFR)")
                                        }).response.on_disabled_hover_text("VFR is disabled for ProRes and DNxHR to prevent NLE timeline desync.");
                                    } else {
                                        ui.radio_value(&mut self.settings.timing, Timing::TrueExifVfr, "True EXIF (Native VFR)")
                                            .on_hover_text("Bakes exact timestamps directly into the file. Perfectly smooth and efficient for web playback, but usually breaks when imported into video editors!");
                                    }
                                    ui.radio_value(&mut self.settings.timing, Timing::TrueExifPeak, format!("True EXIF (Peak ~{} FPS CFR)", peak_fps))
                                        .on_hover_text("Calculates the absolute fastest frame in your burst and sets it as the output base. Might drop the very rare occasional frame");
                                    ui.radio_value(&mut self.settings.timing, Timing::TrueExifHighPrecision, "True EXIF (NLE High CFR):")
                                        .on_hover_text("Forces a high constant framerate output and duplicates frames to hit exact millisecond precision. Produces large files");
                                    ui.add_enabled(self.settings.timing == Timing::TrueExifHighPrecision, egui::DragValue::new(&mut self.settings.high_precision_fps).speed(1.0).range(24..=240).suffix(" FPS"));
                                });
                                ui.horizontal(|ui| {
                                    ui.radio_value(&mut self.settings.timing, Timing::FixedFpsAverage, format!("Fixed (Average {} FPS)", average_fps))
                                        .on_hover_text("Ignores camera stutter and spaces all frames perfectly evenly across the total time of the burst.");
                                });
                                ui.horizontal(|ui| {
                                    ui.radio_value(&mut self.settings.timing, Timing::CustomFps, "Custom:");
                                    ui.add_enabled(self.settings.timing == Timing::CustomFps, egui::DragValue::new(&mut self.settings.custom_fps).speed(1.0).range(1.0..=240.0).suffix(" FPS"));
                                });
                            });
                            ui.end_row();

                            ui.label("");
                            if self.settings.codec == Codec::H264 || self.settings.codec == Codec::HEVC || self.settings.codec == Codec::AV1 {
                                ui.checkbox(&mut self.settings.all_intra, "All-Intra (I-Frames only)")
                                    .on_hover_text("Forces every frame to be a standalone keyframe. Huge file sizes, but heavily reduces artifacting and makes editing in NLEs faster.");
                            } else {
                                ui.add_enabled_ui(false, |ui| {
                                    ui.checkbox(&mut true, "All-Intra (I-Frames only)")
                                }).response.on_disabled_hover_text("ProRes and DNxHR are visually lossless intra-frame codecs by nature, so this is inherently active.");
                            }
                            ui.end_row();
                            if old_settings != self.settings {
                                self.settings.preset = Preset::Custom;
                            }
                        });
                    });
                    
                    ui.add_space(10.0);

                    if ui.button("Generate frames.txt and run FFMPEG").clicked() {
                        let frames_txt_path = folder.join("frames.txt");
                        let mut content = String::from("ffconcat version 1.0\n");
                        
                        let average_fps = if burst.len() > 1 {
                            let total_dur = (*burst.last().unwrap().created() - *burst.first().unwrap().created()).as_seconds_f32().max(0.01);
                            ((burst.len() - 1) as f32 / total_dur).round() as u32
                        } else {
                            30
                        };
                        
                        let fixed_duration = match self.settings.timing {
                            Timing::CustomFps => Some(1.0 / self.settings.custom_fps),
                            Timing::FixedFpsAverage => Some(1.0 / (average_fps as f32)),
                            Timing::TrueExifPeak | Timing::TrueExifHighPrecision | Timing::TrueExifVfr => None,
                        };
                        
                        let mut all_durations = Vec::new();
                        
                        for i in 0..burst.len() {
                            let frame = &burst[i];
                            let name = frame.path().file_name().unwrap().to_string_lossy();
                            content.push_str(&format!("file '{}'\n", name));
                            
                            if i + 1 < burst.len() {
                                let seconds = if let Some(fd) = fixed_duration {
                                    fd
                                } else {
                                    let duration = *burst[i + 1].created() - *frame.created();
                                    duration.as_seconds_f32().max(0.01)
                                };
                                all_durations.push(seconds);
                                content.push_str(&format!("duration {:.3}\n", seconds));
                            } else {
                                let end_dur = fixed_duration.unwrap_or(if all_durations.is_empty() { 0.033 } else { all_durations.last().copied().unwrap() });
                                content.push_str(&format!("duration {:.3}\n", end_dur));
                            }
                        }
                        if std::fs::write(&frames_txt_path, content).is_ok() {
                            let folder_clone = folder.clone();
                            let (progress_tx, progress_rx) = std::sync::mpsc::channel();

                            let settings = self.settings;
                            let burst_clone = burst.clone();

                            let handle = thread::spawn(move || {
                                use std::process::Stdio;
                                use std::io::{BufRead, BufReader};
                                
                                if settings.codec == Codec::FcpXml {
                                    return generate_fcpxml(&burst_clone, settings, peak_fps, fixed_duration, &all_durations, &folder_clone, &progress_tx);
                                }
                                
                                let fps_str = match settings.timing {
                                    Timing::CustomFps => settings.custom_fps.to_string(),
                                    Timing::FixedFpsAverage => average_fps.to_string(),
                                    Timing::TrueExifHighPrecision => settings.high_precision_fps.to_string(),
                                    Timing::TrueExifPeak => peak_fps.to_string(),
                                    Timing::TrueExifVfr => "vfr".to_string(),
                                };
                                
                                let mut ffmpeg_args = vec!["-f", "concat", "-safe", "0", "-i", "frames.txt"];
                                if settings.timing == Timing::TrueExifVfr {
                                    ffmpeg_args.extend(["-fps_mode", "vfr"]);
                                } else {
                                    ffmpeg_args.extend(["-r", &fps_str, "-fps_mode", "cfr"]);
                                }
                                
                                let ext = match settings.codec {
                                    Codec::H264 => {
                                        ffmpeg_args.extend(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart"]);
                                        if settings.all_intra { ffmpeg_args.extend(["-g", "1", "-keyint_min", "1"]); }
                                        "mp4"
                                    },
                                    Codec::HEVC => {
                                        ffmpeg_args.extend(["-c:v", "libx265", "-pix_fmt", "yuv420p", "-movflags", "+faststart"]);
                                        if settings.all_intra { ffmpeg_args.extend(["-x265-params", "keyint=1:min-keyint=1"]); }
                                        "mp4"
                                    },
                                    Codec::AV1 => {
                                        ffmpeg_args.extend(["-c:v", "libsvtav1", "-pix_fmt", "yuv420p10le", "-preset", "6", "-crf", "35", "-movflags", "+faststart"]);
                                        if settings.all_intra { ffmpeg_args.extend(["-g", "1"]); }
                                        "mp4"
                                    },
                                    Codec::ProRes => {
                                        ffmpeg_args.extend(["-c:v", "prores_ks", "-profile:v", "3", "-vendor", "apl0", "-pix_fmt", "yuv422p10le"]);
                                        "mov"
                                    },
                                    Codec::DNxHR => {
                                        ffmpeg_args.extend(["-c:v", "dnxhd", "-profile:v", "dnxhr_hq", "-pix_fmt", "yuv422p"]);
                                        "mov"
                                    },
                                    Codec::FcpXml => unreachable!()
                                };
                                
                                let output_file = format!("output.{}", ext);
                                ffmpeg_args.extend(["-progress", "pipe:1", &output_file, "-y"]);
                                
                                let mut child = std::process::Command::new("ffmpeg")
                                    .current_dir(&folder_clone)
                                    .args(&ffmpeg_args)
                                    .stdout(Stdio::piped())
                                    .stderr(Stdio::null())
                                    .spawn()
                                    .expect("Failed to spawn FFMPEG");

                                if let Some(stdout) = child.stdout.take() {
                                    let reader = BufReader::new(stdout);
                                    let mut out_time = String::new();
                                    let mut frame = String::new();
                                    
                                    for line in reader.lines() {
                                        if let Ok(l) = line {
                                            if l.starts_with("out_time=") {
                                                out_time = l.replace("out_time=", "");
                                            } else if l.starts_with("frame=") {
                                                frame = l.replace("frame=", "");
                                            }
                                            if !out_time.is_empty() && !frame.is_empty() {
                                                let frame_idx = frame.trim().parse::<usize>().unwrap_or(0);
                                                let _ = progress_tx.send((frame_idx, format!("Time: {}", out_time)));
                                            }
                                        }
                                    }
                                }
                                
                                child.wait().map(|s| s.success()).unwrap_or(false)
                            });
                            
                            self.state = ThreaderState::Rendering {
                                folder: folder.clone(),
                                handle,
                                progress_rx,
                                current_progress: "Starting FFMPEG...".to_string(),
                                current_frame: 0,
                                total_frames: burst.len(),
                            };
                        }
                    }
                }
                ThreaderState::Rendering { progress_rx, current_progress, current_frame, total_frames, .. } => {
                    // Drain the channel for the latest progress
                    while let Ok((f_idx, msg)) = progress_rx.try_recv() {
                        *current_frame = f_idx;
                        *current_progress = msg;
                    }
                    
                    let ratio = if *total_frames > 0 { (*current_frame as f32) / (*total_frames as f32) } else { 0.0 };
                    
                    ui.spinner();
                    ui.label("Rendering output.mp4 via FFMPEG...");
                    ui.add_space(10.0);
                    ui.add(egui::ProgressBar::new(ratio).show_percentage());
                    ui.label(format!("{}/{} Frames", current_frame, total_frames));
                    ui.label(current_progress.as_str());
                    ui.ctx().request_repaint();
                }
                ThreaderState::Done { success, folder } => {
                    if *success {
                        let ext = match self.settings.codec {
                            Codec::H264 | Codec::HEVC | Codec::AV1 => "mp4",
                            Codec::ProRes | Codec::DNxHR => "mov",
                            Codec::FcpXml => "xml",
                        };
                        let output_file = folder.join(format!("output.{}", ext));
                        
                        ui.colored_label(egui::Color32::GREEN, "Success!");
                        ui.label(format!("Saved to {}", output_file.display()));
                        ui.add_space(10.0);
                        
                        ui.horizontal(|ui| {
                            if ui.button("Play Video").clicked() {
                                let _ = open::that(&output_file);
                            }
                            if ui.button("Start Over").clicked() {
                                self.state = ThreaderState::Initial(None);
                            }
                        });
                    } else {
                        ui.colored_label(egui::Color32::RED, "FFMPEG Failed.");
                        ui.add_space(10.0);
                        if ui.button("Start Over").clicked() {
                            self.state = ThreaderState::Initial(None);
                        }
                    }
                }
                ThreaderState::Error(err) => {
                    ui.colored_label(egui::Color32::RED, "Error:");
                    ui.label(err.clone());
                    ui.add_space(10.0);
                    if ui.button("Start Over").clicked() {
                        self.state = ThreaderState::Initial(None);
                    }
                }
            }
            });
        });
    }
}

fn generate_fcpxml(
    burst: &[BurstFile],
    settings: RenderSettings,
    peak_fps: u32,
    fixed_duration: Option<f32>,
    all_durations: &[f32],
    folder: &std::path::Path,
    progress_tx: &std::sync::mpsc::Sender<(usize, String)>
) -> bool {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.9\">\n<resources>\n");
    
    let base_fps = if settings.timing == Timing::TrueExifPeak { peak_fps } else { settings.high_precision_fps.max(24) };
    
    let mut width = 1920;
    let mut height = 1080;
    if let Some(first_frame) = burst.first() {
        if let Ok((w, h)) = image::image_dimensions(first_frame.path()) {
            width = w;
            height = h;
        }
    }
    
    xml.push_str(&format!("  <format id=\"r0\" name=\"ThreaderFormat\" frameDuration=\"1/{}s\" width=\"{}\" height=\"{}\"/>\n", base_fps, width, height));
    
    for (i, frame) in burst.iter().enumerate() {
        let abs_path = std::fs::canonicalize(frame.path()).unwrap_or_else(|_| frame.path().clone());
        let path_str = abs_path.to_string_lossy().replace("\\", "/");
        let file_url = if path_str.starts_with('/') {
            format!("file://{}", path_str)
        } else {
            format!("file:///{}", path_str)
        };
        let name = frame.path().file_name().unwrap().to_string_lossy();
        let name = name.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;").replace("'", "&apos;");
        xml.push_str(&format!("  <asset id=\"r{}\" name=\"{}\" src=\"{}\" />\n", i + 1, name, file_url));
    }
    xml.push_str("</resources>\n<library>\n<event name=\"Burst Event\">\n<project name=\"Threader Burst\">\n<sequence format=\"r0\">\n<spine>\n");
    
    let mut current_offset_frames: u64 = 0;
    
    for i in 0..burst.len() {
        let frame = &burst[i];
        
        let seconds = if let Some(fd) = fixed_duration {
            fd
        } else if i + 1 < burst.len() {
            let duration = *burst[i + 1].created() - *frame.created();
            duration.as_seconds_f32().max(0.01)
        } else {
            if all_durations.is_empty() { 0.033 } else { all_durations.last().copied().unwrap_or(0.033) }
        };
        
        let duration_frames = (seconds * base_fps as f32).round().max(1.0) as u64;
        
        let name = frame.path().file_name().unwrap().to_string_lossy();
        let name = name.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;").replace("'", "&apos;");
        
        xml.push_str(&format!("  <video name=\"{}\" ref=\"r{}\" offset=\"{}/{}s\" duration=\"{}/{}s\" start=\"0s\"/>\n", 
            name,
            i + 1,
            current_offset_frames, base_fps,
            duration_frames, base_fps
        ));
        
        current_offset_frames += duration_frames;
        if i % 10 == 0 {
            let _ = progress_tx.send((i, format!("Generating FCPXML... {}/{}", i, burst.len())));
        }
    }
    
    xml.push_str("</spine>\n</sequence>\n</project>\n</event>\n</library>\n</fcpxml>");
    
    let output_file = folder.join("output.xml");
    let _ = std::fs::write(&output_file, xml);
    let _ = progress_tx.send((burst.len(), "Done".to_string()));
    true
}
