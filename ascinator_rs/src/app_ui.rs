use crate::ascii_converter::{self, get_char_sets, ConversionOptions, AsciiFrameData};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender, Receiver}; // For thread communication
use std::thread;

// Enum to represent the active tab
#[derive(PartialEq)]
enum Tab {
    Converter,
    Player,
}

// Messages for communication between worker thread and GUI thread
enum WorkerMessage {
    Progress(f32, String), // progress percentage, status message
    ConversionDone(Result<AsciiFrameData, String>, PathBuf), // Updated to send full AsciiFrameData, original video path for context
    // Audio messages can be added later
}

pub struct AsciiApp {
    active_tab: Tab,

    // --- Converter Tab State ---
    converter_video_path: Option<PathBuf>,
    converter_video_path_str: String, // For display
    converter_output_width: i32,
    converter_selected_charset: String,
    char_set_names: Vec<String>, // To populate combobox
    conversion_in_progress: bool,
    conversion_progress: f32, // 0.0 to 1.0
    conversion_status_message: String,
    extract_audio_flag: bool,

    // Worker thread communication
    worker_tx: Option<Sender<WorkerMessage>>, // To send messages to worker (e.g. cancel) - not used yet
    worker_rx: Option<Receiver<WorkerMessage>>, // To receive messages from worker

    // --- Player Tab State (Placeholders for now) ---
    player_ascii_file_path: Option<PathBuf>,
    player_ascii_file_path_str: String,
    loaded_ascii_data: Option<AsciiFrameData>,
    player_status_message: String,

    current_playback_frame_index: usize,
    playback_paused: bool,
    player_target_fps: f64, // FPS for playback, can be different from original
    last_frame_time: Option<std::time::Instant>,

    // Audio playback (rodio)
    // Option because we only initialize it when needed and it can't be created in new() easily
    // as it might involve thread spawning for the output stream.
    audio_output_stream: Option<rodio::OutputStream>,
    audio_sink: Option<rodio::Sink>,
    // We need to keep the stream handle alive, even if not used directly after creation.
    _audio_stream_handle: Option<rodio::OutputStreamHandle>,
}

impl AsciiApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let char_sets = get_char_sets();
        let char_set_names: Vec<String> = char_sets.keys().cloned().collect();
        let default_charset = char_set_names.first().cloned().unwrap_or_else(|| "Standard".to_string());

        Self {
            active_tab: Tab::Converter,
            converter_video_path: None,
            converter_video_path_str: "No file selected.".to_string(),
            converter_output_width: 120,
            converter_selected_charset: default_charset,
            char_set_names,
            conversion_in_progress: false,
            conversion_progress: 0.0,
            conversion_status_message: "Ready.".to_string(),
            extract_audio_flag: false,

            worker_tx: None,
            worker_rx: None,

            player_ascii_file_path: None,
            player_ascii_file_path_str: "No file loaded.".to_string(),
            loaded_ascii_data: None,
            player_status_message: "Load an ASCII video file to play.".to_string(),
            current_playback_frame_index: 0,
            playback_paused: true,
            player_target_fps: 30.0, // Default playback FPS
            last_frame_time: None,

            audio_output_stream: None,
            audio_sink: None,
            _audio_stream_handle: None,
        }
    }

    fn reset_player_state(&mut self) {
        self.current_playback_frame_index = 0;
        self.playback_paused = true;
        self.last_frame_time = None;
        if let Some(sink) = &self.audio_sink {
            sink.stop();
        }
        // self.audio_sink = None; // Cleared when new audio is loaded or file is unloaded
        // self.audio_output_stream = None; // This one is harder to reset, typically created once.
    }

    fn load_ascii_file_for_player(&mut self) {
        // Stop any existing audio before loading a new file
        if let Some(sink) = &self.audio_sink {
            sink.stop();
        }
        self.audio_sink = None; // Clear the old sink

        if let Some(path) = rfd::FileDialog::new()
            .add_filter("ASCII Video Files", &["ascii_vid"])
            .pick_file()
        {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    match serde_pickle::from_slice::<AsciiFrameData>(&bytes) {
                        Ok(loaded_data) => {
                            // Set player FPS to original video FPS by default, if available
                            self.player_target_fps = loaded_data.fps;
                            self.reset_player_state(); // Reset before loading new data

                            self.player_ascii_file_path = Some(path.clone());
                            self.player_ascii_file_path_str = path.display().to_string();
                            self.player_status_message = format!("Loaded: {}", path.file_name().unwrap_or_default().to_string_lossy());

                            // Audio Setup
                            if let Some(audio_file_p) = &loaded_data.audio_path {
                                if self.audio_output_stream.is_none() {
                                    // Try to initialize audio stream and handle if not already done
                                    match rodio::OutputStream::try_default() {
                                        Ok((stream, handle)) => {
                                            self.audio_output_stream = Some(stream);
                                            self._audio_stream_handle = Some(handle);
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to create audio output stream: {}", e);
                                            self.player_status_message.push_str(&format!(" (Audio stream error: {})", e));
                                        }
                                    }
                                }

                                if let Some(handle) = &self._audio_stream_handle {
                                    match rodio::Sink::try_new(handle) {
                                        Ok(sink) => {
                                            match std::fs::File::open(audio_file_p) {
                                                Ok(file) => {
                                                    match rodio::Decoder::new(std::io::BufReader::new(file)) {
                                                        Ok(decoder) => {
                                                            sink.append(decoder);
                                                            sink.pause(); // Start paused, will play on play button
                                                            self.audio_sink = Some(sink);
                                                            self.player_status_message.push_str(" (Audio linked)");
                                                        }
                                                        Err(e) => {
                                                            eprintln!("Failed to decode audio file {:?}: {}", audio_file_p, e);
                                                            self.player_status_message.push_str(&format!(" (Audio decode error: {})", e));
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!("Failed to open audio file {:?}: {}", audio_file_p, e);
                                                    self.player_status_message.push_str(&format!(" (Audio file error: {})", e));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to create audio sink: {}", e);
                                            self.player_status_message.push_str(&format!(" (Audio sink error: {})", e));
                                        }
                                    }
                                }
                            }
                            self.loaded_ascii_data = Some(loaded_data); // Assign last after all setup for it is done.
                        }
                        Err(e) => {
                            self.reset_player_state();
                            self.loaded_ascii_data = None; // Clear data on error
                            self.player_status_message = format!("Error deserializing file: {}", e);
                            eprintln!("Error deserializing file {}: {}", path.display(), e);
                        }
                    }
                }
                Err(e) => {
                    self.loaded_ascii_data = None;
                    self.player_status_message = format!("Error reading file: {}", e);
                    eprintln!("Error reading file {}: {}", path.display(), e);
                }
            }
        } else {
            // User cancelled dialog - no change in status needed unless providing feedback
            // self.player_status_message = "File load cancelled.".to_string();
        }
    }

    fn ui_converter_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Video to ASCII Converter");
        ui.separator();

        // 1. Select Video
        ui.label("1. Select Video");
        ui.horizontal(|ui| {
            if ui.button("Select Video File").clicked() && !self.conversion_in_progress {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Video Files", &["mp4", "avi", "mov", "mkv"])
                    .pick_file()
                {
                    self.converter_video_path = Some(path.clone());
                    self.converter_video_path_str = path.display().to_string();
                    self.conversion_status_message = format!("Selected: {}", path.file_name().unwrap_or_default().to_string_lossy());
                }
            }
            ui.label(&self.converter_video_path_str);
        });
        ui.add_space(10.0);

        // 2. Set Options
        ui.label("2. Set Options");
        egui::Grid::new("converter_options_grid")
            .num_columns(2)
            .spacing([20.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Output Width (chars):");
                ui.add_enabled(!self.conversion_in_progress, egui::DragValue::new(&mut self.converter_output_width).clamp_range(40..=500));
                ui.end_row();

                ui.label("Character Set:");
                egui::ComboBox::from_id_source("charset_select")
                    .selected_text(self.converter_selected_charset.clone())
                    .show_ui(ui, |ui| {
                        for name in &self.char_set_names {
                            ui.selectable_value(&mut self.converter_selected_charset, name.clone(), name);
                        }
                    });
                ui.end_row();

                ui.label("Extract Audio:");
                ui.add_enabled(!self.conversion_in_progress, egui::Checkbox::new(&mut self.extract_audio_flag, "Extract and link audio track"));
                ui.end_row();
            });
        ui.add_space(10.0);

        // 3. Convert
        ui.label("3. Convert");
        if ui.add_enabled(!self.conversion_in_progress && self.converter_video_path.is_some(), egui::Button::new("Start Conversion")).clicked() {
            self.start_conversion();
        }
        ui.add_space(10.0);

        // Status
        ui.label("Status");
        ui.add(egui::ProgressBar::new(self.conversion_progress).show_percentage());
        ui.label(&self.conversion_status_message);

        // Check for messages from worker thread
        if let Some(rx) = &self.worker_rx {
            match rx.try_recv() {
                Ok(WorkerMessage::Progress(val, msg)) => {
                    self.conversion_progress = val / 100.0; // Assuming val is 0-100
                    self.conversion_status_message = msg;
                }
                Ok(WorkerMessage::ConversionDone(Ok(ascii_data), source_video_path)) => {
                    self.conversion_in_progress = false;
                    self.conversion_progress = 1.0;
                    self.conversion_status_message = "Conversion complete! Ready to save.".to_string();
                    // Now trigger save dialog
                    self.save_ascii_file(ascii_data, source_video_path);
                }
                Ok(WorkerMessage::ConversionDone(Err(err_msg), _)) => {
                    self.conversion_in_progress = false;
                    // Display error more prominently in UI if possible, for now, status label is it.
                    // Using RichText for color would require changing how status_label is rendered.
                    // For now, we'll stick to the string and rely on eprintln for dev.
                    // A potential UI improvement: self.conversion_status_is_error = true; and color label based on it.
                    self.conversion_status_message = format!("ERROR: {}", err_msg); // Prefix with ERROR
                    eprintln!("Conversion Error: {}", err_msg);
                }
                Err(mpsc::TryRecvError::Empty) => { /* No message, do nothing */ }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.conversion_in_progress = false;
                    self.conversion_status_message = "Worker thread disconnected unexpectedly.".to_string();
                    self.worker_rx = None; // Stop trying to receive
                }
            }
        }
    }

    fn start_conversion(&mut self) {
        if self.conversion_in_progress { return; }

        guard!(let Some(video_path_buf) = self.converter_video_path.clone() else {
            self.conversion_status_message = "Error: Video path is not set.".to_string();
            return;
        });

        let video_path = video_path_buf.to_string_lossy().to_string(); // Path for the worker

        let options = ConversionOptions {
            width: self.converter_output_width,
            char_set_name: self.converter_selected_charset.clone(),
        };
        // let extract_audio = self.extract_audio_flag; // Will be used later

        let (tx_worker_to_gui, rx_gui_from_worker) = mpsc::channel();
        // Store receiver for GUI to listen on
        self.worker_rx = Some(rx_gui_from_worker);
        // self.worker_tx will be for GUI to Worker, if needed (e.g. cancellation)

        self.conversion_in_progress = true;
        self.conversion_progress = 0.0;
        self.conversion_status_message = "Initializing conversion...".to_string();

        let ffmpeg_path_str = "C:\\Users\\artur\\Desktop\\ascinator\\ffmpeg\\ffmpeg.exe".to_string(); // As per user
        let extract_audio = self.extract_audio_flag;

        // --- Worker Thread ---
        thread::spawn(move || {
            // 1. (Optional) Audio Extraction
            let mut audio_file_path: Option<PathBuf> = None;
            if extract_audio {
                let source_path_for_ffmpeg = PathBuf::from(&video_path);
                let audio_output_filename = source_path_for_ffmpeg.file_stem().unwrap_or_default().to_string_lossy().to_string() + "_audio.aac";
                // For now, save audio next to the video. Could be configurable.
                let audio_output_path = source_path_for_ffmpeg.with_file_name(audio_output_filename);

                tx_worker_to_gui.send(WorkerMessage::Progress(0.0, "Extracting audio...".to_string())).unwrap_or_default();

                let mut cmd = std::process::Command::new(&ffmpeg_path_str);
                cmd.arg("-i").arg(&video_path)
                   .arg("-vn") // No video
                   .arg("-acodec")
                   .arg("copy") // Copy audio stream without re-encoding
                   .arg("-y") // Overwrite output file if it exists
                   .arg(&audio_output_path);

                // IMPORTANT: Path to ffmpeg as specified by the user.
                // For a production app, this should be configurable or detected.
                let ffmpeg_executable_path_for_error = ffmpeg_path_str.clone(); // For error message

                match cmd.output() { // Use output() to capture stderr
                    Ok(output) if output.status.success() => {
                        tx_worker_to_gui.send(WorkerMessage::Progress(0.0, "Audio extracted successfully.".to_string())).unwrap_or_default();
                        audio_file_path = Some(audio_output_path);
                    }
                    Ok(output) => { // FFmpeg ran but indicated failure
                        let stderr_output = String::from_utf8_lossy(&output.stderr);
                        let user_friendly_msg = "FFmpeg audio extraction failed. Check console log for details.".to_string();
                        let detailed_err_msg = format!(
                            "FFmpeg audio extraction failed with status: {}. FFmpeg path: '{}'. Stderr:\n{}",
                            output.status,
                            ffmpeg_executable_path_for_error, // Show path in detailed log
                            stderr_output.trim()
                        );
                        tx_worker_to_gui.send(WorkerMessage::Progress(0.0, user_friendly_msg)).unwrap_or_default();
                        eprintln!("{}", detailed_err_msg); // Log detailed error to console
                    }
                    Err(e) => { // Failed to start FFmpeg
                        let user_friendly_msg = "Failed to execute FFmpeg. Check console log for details.".to_string();
                        let detailed_err_msg = format!("Failed to execute FFmpeg (path: {}): {}. Ensure FFmpeg is installed and path is correct.", ffmpeg_executable_path_for_error, e);
                        tx_worker_to_gui.send(WorkerMessage::Progress(0.0, err_msg.clone())).unwrap_or_default();
                        eprintln!("{}", err_msg);
                    }
                }
            }


            // 2. ASCII Conversion
            let conversion_result = ascii_converter::video_to_ascii_frames(
                &video_path,
                &options,
                // Some(Box::new({ // Progress callback
                //     let tx_clone = tx_worker_to_gui.clone();
                //     move |progress, message| {
                //         tx_clone.send(WorkerMessage::Progress(progress, message)).unwrap_or_default();
                //     }
                // }))
            );

            // Augment AsciiFrameData with audio_file_path if successful
            let final_result = match conversion_result {
                Ok(mut data) => {
                    // This part is tricky because AsciiFrameData doesn't have audio_path yet.
                    // We'll need to handle this when saving, or modify AsciiFrameData.
                    // For now, we pass audio_file_path separately to the save function via the message.
                    // Let's assume ConversionDone will carry this.
                    // The plan was to modify AsciiFrameData, let's assume we do that conceptually for now.
                    // For the purpose of this message, we'll just send the core data and handle audio path later.
                    Ok(data)
                }
                Err(e) => Err(e),
            };

            // Augment AsciiFrameData with audio_file_path if successful
            let final_result_with_audio = match conversion_result {
                Ok(mut ascii_data_core) => {
                    // Now that AsciiFrameData has audio_path, set it.
                    ascii_data_core.audio_path = audio_file_path; // audio_file_path is Option<PathBuf> from audio extraction
                    Ok(ascii_data_core)
                }
                Err(e) => Err(e),
            };

            let original_video_path_for_context = PathBuf::from(video_path);
            tx_worker_to_gui.send(WorkerMessage::ConversionDone(final_result_with_audio, original_video_path_for_context)).unwrap_or_default();
        });
    }

    fn save_ascii_file(&mut self, data_to_save: AsciiFrameData, source_video_path_context: PathBuf) {
        // Determine default output filename
        let default_filename = source_video_path_context // Use the context path for naming suggestion
            .file_stem().unwrap_or_default().to_string_lossy().to_string() + ".ascii_vid";

        if let Some(save_path) = rfd::FileDialog::new()
            .set_file_name(&default_filename)
            .add_filter("ASCII Video Files", &["ascii_vid"])
            .save_file()
        {
            // Now data_to_save is already the complete AsciiFrameData struct with audio_path
            match serde_pickle::to_vec(&data_to_save, true) {
                Ok(bytes) => {
                    match std::fs::write(&save_path, bytes) {
                        Ok(_) => {
                            self.conversion_status_message = format!("File saved to: {}", save_path.display());
                            if let Some(audio_p) = &data_to_save.audio_path {
                                self.conversion_status_message.push_str(&format!(" (Audio linked: {})", audio_p.display()));
                            }
                        }
                        Err(e) => {
                            self.conversion_status_message = format!("Error writing file: {}", e);
                            eprintln!("Error writing file {}: {}", save_path.display(), e);
                        }
                    }
                }
                Err(e) => {
                    self.conversion_status_message = format!("Error serializing data: {}", e);
                    eprintln!("Error serializing data: {}", e);
                    // Potentially try bincode as a fallback if pickle fails? For now, just error.
                }
            }
        } else {
            self.conversion_status_message = "Save operation cancelled.".to_string();
        }
    }


    fn ui_player_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("ASCII Player");
        ui.separator();

        // --- ASCII File Loading ---
        ui.horizontal(|ui| {
            if ui.button("Load ASCII Video (.ascii_vid)").clicked() {
                self.load_ascii_file_for_player();
            }
            ui.label(&self.player_ascii_file_path_str);
        });
        ui.label(&self.player_status_message);
        ui.separator();

        if let Some(data) = &self.loaded_ascii_data {
            // --- Playback Screen ---
            // Use a ScrollArea for potentially large ASCII frames
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                // To ensure monospace, we should ideally use egui::TextFormat and RichText,
                // but for simplicity, Label with a monospace font name *might* work on some systems.
                // A more robust way is to use egui::FontId.
                let text_style = egui::TextStyle::Monospace;
                let font_id = text_style.resolve(ui.style());
                // Fallback if Monospace isn't found, though it should be.
                // let font_id = egui::FontId::new(10.0, egui::FontFamily::Monospace);

                if self.current_playback_frame_index < data.frames.len() {
                    ui.label(egui::RichText::new(&data.frames[self.current_playback_frame_index]).font(font_id));
                } else {
                    ui.label(egui::RichText::new("End of video or no frame.").font(font_id));
                }
            });
            ui.separator();

            // --- Playback Controls ---
            ui.horizontal(|ui| {
                // Play/Pause Button
                let play_pause_text = if self.playback_paused { "▶ Play" } else { "❚❚ Pause" };
                if ui.button(play_pause_text).clicked() {
                    self.playback_paused = !self.playback_paused;
                    if !self.playback_paused {
                        self.last_frame_time = Some(std::time::Instant::now()); // Reset timer on play
                        if let Some(sink) = &self.audio_sink {
                            sink.play();
                        }
                    } else {
                        if let Some(sink) = &self.audio_sink {
                            sink.pause();
                        }
                    }
                }

                // Slider for scrubbing
                let num_frames = data.frames.len();
                if num_frames > 0 {
                    let mut slider_frame = self.current_playback_frame_index as f32;
                    if ui.add(egui::Slider::new(&mut slider_frame, 0.0..=(num_frames -1) as f32).text("Frame")).changed() {
                        self.current_playback_frame_index = slider_frame.round() as usize;
                        self.last_frame_time = Some(std::time::Instant::now()); // Reset timer on seek
                        if let Some(sink) = &self.audio_sink {
                            // Seeking audio is complex with basic rodio sink.
                            // For simplicity, we might restart audio or just let it play.
                            // Restarting is more accurate but can be jarring.
                            // For now, let's assume audio continues or we handle it later.
                            // A more advanced audio library or direct ffmpeg playback would be needed for precise audio seeking.
                            // For now, we will just update the visual frame. If playing, audio will continue from its current position.
                            // If paused and then scrubbed, then played, audio will resume.
                        }
                    }
                }

                // FPS input
                ui.label("FPS:");
                ui.add(egui::DragValue::new(&mut self.player_target_fps).speed(0.1).clamp_range(1.0..=240.0));
            });

            // Frame counter
            ui.label(format!("Frame: {} / {}", self.current_playback_frame_index + 1, data.frames.len()));


            // --- Playback Logic (called from main update loop via request_repaint) ---
            if !self.playback_paused && !data.frames.is_empty() {
                let now = std::time::Instant::now();
                let frame_duration = std::time::Duration::from_secs_f64(1.0 / self.player_target_fps.max(1.0));

                if let Some(last_time) = self.last_frame_time {
                    if now.duration_since(last_time) >= frame_duration {
                        self.current_playback_frame_index += 1;
                        if self.current_playback_frame_index >= data.frames.len() {
                            self.current_playback_frame_index = 0; // Loop
                            if let Some(sink) = &self.audio_sink { // Restart audio on loop
                                sink.stop(); // Stop current
                                sink.clear(); // Remove completed audio source
                                // Re-add the audio source to the sink to allow looping
                                if let Some(audio_file_p) = &data.audio_path {
                                     if let Ok(file) = std::fs::File::open(audio_file_p) {
                                        if let Ok(decoder) = rodio::Decoder::new(std::io::BufReader::new(file)) {
                                            sink.append(decoder);
                                            sink.play();
                                        }
                                     }
                                }
                            }
                        }
                        self.last_frame_time = Some(now);
                    }
                } else {
                    // First frame after unpausing
                    self.last_frame_time = Some(now);
                }
            }

        } else {
            ui.label("No ASCII data loaded.");
        }
    }
}

// Helper macro for guards, from egui examples or common Rust practice
macro_rules! guard {
    ($cond:expr else $fallback:block) => {
        if !$cond {
            $fallback
        }
    };
    (let Some($pat:pat) = $expr:expr else $fallback:block) => {
        let Some($pat) = $expr else {
            $fallback
        };
    };
}


impl eframe::App for AsciiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Converter, "Convert Video");
                ui.selectable_value(&mut self.active_tab, Tab::Player, "Play ASCII File");
            });
            ui.separator();

            match self.active_tab {
                Tab::Converter => {
                    self.ui_converter_tab(ui);
                }
                Tab::Player => {
                    self.ui_player_tab(ui);
                }
            }
        });
        // Request repaint for animations or continuous updates (e.g., progress, playback)
        ctx.request_repaint();
    }
}
