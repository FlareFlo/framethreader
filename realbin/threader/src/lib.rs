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
        progress_rx: std::sync::mpsc::Receiver<String>,
        current_progress: String,
    },
    Done {
        folder: PathBuf,
        success: bool,
    },
    Error(String),
}

struct ThreaderApp {
    state: ThreaderState,
}

impl ThreaderApp {
    fn new(folder: Option<PathBuf>) -> Self {
        Self {
            state: ThreaderState::Initial(folder),
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

                    if ui.button("Generate frames.txt and run FFMPEG").clicked() {
                        let frames_txt_path = folder.join("frames.txt");
                        let mut content = String::from("ffconcat version 1.0\n");
                        
                        // Calculate durations for each frame to keep true playback speed
                        for i in 0..burst.len() {
                            let frame = &burst[i];
                            let name = frame.path().file_name().unwrap().to_string_lossy();
                            content.push_str(&format!("file '{}'\n", name));
                            
                            // If it's not the last frame, calculate duration to the next frame
                            if i + 1 < burst.len() {
                                let duration = *burst[i + 1].created() - *frame.created();
                                // Clamp to a sensible fallback if timestamps are identical (e.g. 30fps = 0.033)
                                let seconds = duration.as_seconds_f32().max(0.01);
                                content.push_str(&format!("duration {:.3}\n", seconds));
                            } else {
                                // Last frame gets a small standard duration
                                content.push_str("duration 0.033\n");
                            }
                        }
                        
                        if std::fs::write(&frames_txt_path, content).is_ok() {
                            let folder_clone = folder.clone();
                            let (progress_tx, progress_rx) = std::sync::mpsc::channel();
                            
                            let handle = thread::spawn(move || {
                                use std::process::Stdio;
                                use std::io::{BufRead, BufReader};
                                
                                let mut child = std::process::Command::new("ffmpeg")
                                    .current_dir(&folder_clone)
                                    .args(["-f", "concat", "-safe", "0", "-i", "frames.txt", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-progress", "pipe:1", "output.mp4", "-y"])
                                    .stdout(Stdio::piped())
                                    .stderr(Stdio::null()) // suppress normal stderr logs to not pollute console
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
                                                let _ = progress_tx.send(format!("Frame: {} | Time: {}", frame, out_time));
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
                            };
                        }
                    }
                }
                ThreaderState::Rendering { progress_rx, current_progress, .. } => {
                    // Drain the channel for the latest progress
                    while let Ok(msg) = progress_rx.try_recv() {
                        *current_progress = msg;
                    }
                    
                    ui.spinner();
                    ui.label("Rendering output.mp4 via FFMPEG...");
                    ui.label(current_progress.as_str());
                    ui.ctx().request_repaint();
                }
                ThreaderState::Done { success, folder } => {
                    if *success {
                        ui.colored_label(egui::Color32::GREEN, "Success!");
                        ui.label(format!("Saved to {}/output.mp4", folder.display()));
                    } else {
                        ui.colored_label(egui::Color32::RED, "FFMPEG Failed.");
                    }
                    ui.add_space(10.0);
                    if ui.button("Start Over").clicked() {
                        self.state = ThreaderState::Initial(None);
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
    }
}
