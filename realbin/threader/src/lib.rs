use std::path::PathBuf;
use eframe::egui;

pub const RUNMODE: &str = "threader";

pub fn realmain() {
    // Try to get folder from env variable or argument
    let path = std::env::args().nth(1).map(PathBuf::from);
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Threader",
        options,
        Box::new(|_cc| Ok(Box::new(ThreaderApp::new(path)))),
    ).unwrap();
}

struct ThreaderApp {
    folder: Option<PathBuf>,
}

impl ThreaderApp {
    fn new(folder: Option<PathBuf>) -> Self {
        Self { folder }
    }
}

impl eframe::App for ThreaderApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Threader - FFMPEG Burst Processor");
            
            ui.add_space(20.0);
            
            if let Some(folder) = &self.folder {
                ui.label(format!("Selected Folder: {}", folder.display()));
                ui.add_space(10.0);
                
                if ui.button("Start FFMPEG processing (Stub)").clicked() {
                    println!("Would process {}", folder.display());
                }
            } else {
                ui.label("No folder selected.");
                ui.add_space(10.0);
                if ui.button("Select Folder").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.folder = Some(path);
                    }
                }
            }
        });
    }
}
