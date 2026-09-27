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

#[derive(PartialEq, Clone, Copy)]
enum Codec {
    H264,
    HEVC,
    ProRes,
    AV1,
}

#[derive(PartialEq, Clone, Copy)]
enum Timing {
    TrueExifDynamic,
    TrueExifHighPrecision,
    FixedFpsAverage,
    FixedFps(u32),
    CustomFps,
}

struct RenderSettings {
    codec: Codec,
    timing: Timing,
    custom_fps: f32,
    all_intra: bool, // Force I-frames for H264/HEVC
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            codec: Codec::H264,
            timing: Timing::TrueExifDynamic,
            custom_fps: 120.0,
            all_intra: false,
        }
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
                    
                    ui.group(|ui| {
                        ui.heading("Output Settings");
                        ui.horizontal(|ui| {
                            ui.label("Codec:");
                            ui.radio_value(&mut self.settings.codec, Codec::H264, "H.264 (MP4)")
                                .on_hover_text("Universally compatible and widely supported. Great for general sharing and web upload.");
                            ui.radio_value(&mut self.settings.codec, Codec::HEVC, "HEVC (MP4)")
                                .on_hover_text("High Efficiency Video Coding. Yields much smaller file sizes than H.264, but requires newer hardware to play back.");
                            ui.radio_value(&mut self.settings.codec, Codec::AV1, "AV1 (MP4)")
                                .on_hover_text("Next-generation open codec. Unbeatable file sizes and quality, but encoding is extremely slow.");
                            ui.radio_value(&mut self.settings.codec, Codec::ProRes, "ProRes (MOV)")
                                .on_hover_text("Visually lossless, all-intra codec. Best for importing into video editors like Premiere or Resolve.");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Timing:");
                            ui.radio_value(&mut self.settings.timing, Timing::TrueExifDynamic, "True EXIF (Dynamic CFR)")
                                .on_hover_text("Calculates the median framerate of your burst and sets it as the output base to minimize duplicated frames while keeping timing roughly accurate.");
                            ui.radio_value(&mut self.settings.timing, Timing::TrueExifHighPrecision, "True EXIF (120FPS CFR)")
                                .on_hover_text("Forces a flat 120 FPS output and duplicates frames to hit exact millisecond precision. Will result in massive file sizes for ProRes!");
                            ui.radio_value(&mut self.settings.timing, Timing::FixedFpsAverage, "Fixed (Average FPS)")
                                .on_hover_text("Ignores camera stutter and spaces all frames perfectly evenly across the total time of the burst.");
                            ui.radio_value(&mut self.settings.timing, Timing::FixedFps(24), "Fixed 24 FPS");
                            ui.radio_value(&mut self.settings.timing, Timing::FixedFps(30), "Fixed 30 FPS");
                            ui.radio_value(&mut self.settings.timing, Timing::FixedFps(60), "Fixed 60 FPS");
                            ui.radio_value(&mut self.settings.timing, Timing::CustomFps, "Custom:");
                            if self.settings.timing == Timing::CustomFps {
                                ui.add(egui::DragValue::new(&mut self.settings.custom_fps).speed(1.0).range(1.0..=240.0).suffix(" FPS"));
                            }
                        });
                        if self.settings.codec == Codec::H264 || self.settings.codec == Codec::HEVC || self.settings.codec == Codec::AV1 {
                            ui.checkbox(&mut self.settings.all_intra, "All-Intra (I-Frames only)")
                                .on_hover_text("Forces every frame to be a standalone keyframe (like ProRes). Huge file sizes, but heavily reduces artifacting and makes editing in NLEs smooth.");
                        }
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
                            Timing::FixedFps(fps) => Some(1.0 / (fps as f32)),
                            Timing::CustomFps => Some(1.0 / self.settings.custom_fps),
                            Timing::FixedFpsAverage => Some(1.0 / (average_fps as f32)),
                            Timing::TrueExifDynamic | Timing::TrueExifHighPrecision => None,
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
                        
                        let dynamic_fps = if all_durations.is_empty() {
                            30
                        } else {
                            all_durations.sort_by(|a, b| a.partial_cmp(b).unwrap());
                            let median = all_durations[all_durations.len() / 2];
                            (1.0 / median).round() as u32
                        };
                        
                        if std::fs::write(&frames_txt_path, content).is_ok() {
                            let folder_clone = folder.clone();
                            let (progress_tx, progress_rx) = std::sync::mpsc::channel();
                            
                            let s_codec = self.settings.codec;
                            let s_intra = self.settings.all_intra;
                            let s_timing = self.settings.timing;
                            let s_custom_fps = self.settings.custom_fps;
                            
                            let handle = thread::spawn(move || {
                                use std::process::Stdio;
                                use std::io::{BufRead, BufReader};
                                
                                let fps_str = match s_timing {
                                    Timing::FixedFps(fps) => fps.to_string(),
                                    Timing::CustomFps => s_custom_fps.to_string(),
                                    Timing::FixedFpsAverage => average_fps.to_string(),
                                    Timing::TrueExifHighPrecision => "120".to_string(), // 120 FPS CFR for NLEs to digest VFR accurately
                                    Timing::TrueExifDynamic => dynamic_fps.to_string(),
                                };
                                
                                let mut ffmpeg_args = vec!["-f", "concat", "-safe", "0", "-i", "frames.txt", "-r", &fps_str, "-fps_mode", "cfr"];
                                
                                let ext = match s_codec {
                                    Codec::H264 => {
                                        ffmpeg_args.extend(["-c:v", "libx264", "-pix_fmt", "yuv420p"]);
                                        if s_intra { ffmpeg_args.extend(["-g", "1", "-keyint_min", "1"]); }
                                        "mp4"
                                    },
                                    Codec::HEVC => {
                                        ffmpeg_args.extend(["-c:v", "libx265", "-pix_fmt", "yuv420p"]);
                                        if s_intra { ffmpeg_args.extend(["-x265-params", "keyint=1:min-keyint=1"]); }
                                        "mp4"
                                    },
                                    Codec::AV1 => {
                                        ffmpeg_args.extend(["-c:v", "libsvtav1", "-pix_fmt", "yuv420p10le", "-preset", "6", "-crf", "35"]);
                                        if s_intra { ffmpeg_args.extend(["-g", "1"]); }
                                        "mp4"
                                    },
                                    Codec::ProRes => {
                                        ffmpeg_args.extend(["-c:v", "prores_ks", "-profile:v", "3", "-vendor", "apl0", "-pix_fmt", "yuv422p10le"]);
                                        "mov"
                                    }
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
                            Codec::ProRes => "mov",
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
