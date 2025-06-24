import tkinter as tk
from tkinter import ttk, filedialog, messagebox
import cv2
import numpy as np
from PIL import Image
import pickle
import threading
import queue
import time
import os

# --- Character Sets ---
# Different character sets for rendering, from simple to more detailed.
CHAR_SETS = {
    "Standard": "@%#*+=-:. ",
    "Detailed": "$@B%8&WM#*oahkbdpqwmZO0QLCJUYXzcvunxrjft/\\|()1{}[]?-_+~<>i!lI;:,\"^`'. ",
    "Blocks": "█▇▆▅▄▃▂  ",
    "Simple": "#=-. ",
    "Gradient": " .:!/r(l1Z4H9W8$@"[::-1], # Inverted for dark background
}

# --- Main Application Class ---

class AsciiVideoPlayer(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("ASCII Video Converter & Player")
        self.geometry("800x650")

        # --- Member Variables ---
        self.video_path = ""
        self.ascii_data = None
        self.playback_paused = True
        self.playback_job = None
        self.current_frame_index = 0
        self.worker_thread = None
        self.progress_queue = queue.Queue()

        # --- Style and Main Layout ---
        style = ttk.Style(self)
        style.theme_use('clam')
        self.monospace_font = ('Courier New', 8) # A good monospace font is crucial

        # Notebook for tabs
        notebook = ttk.Notebook(self)
        notebook.pack(pady=10, padx=10, expand=True, fill='both')

        # Create tabs
        self.converter_frame = ttk.Frame(notebook)
        self.player_frame = ttk.Frame(notebook)

        notebook.add(self.converter_frame, text='Convert Video')
        notebook.add(self.player_frame, text='Play ASCII File')

        # Build UI for each tab
        self._create_converter_ui()
        self._create_player_ui()

        # Start periodic check for progress queue
        self.after(100, self._check_queue)

    def _create_converter_ui(self):
        frame = self.converter_frame
        
        # --- File Selection ---
        controls_frame = ttk.LabelFrame(frame, text="1. Select Video")
        controls_frame.pack(padx=10, pady=10, fill='x')

        self.select_btn = ttk.Button(controls_frame, text="Select Video File", command=self._select_video_file)
        self.select_btn.pack(side='left', padx=5, pady=5)
        self.video_path_label = ttk.Label(controls_frame, text="No file selected.")
        self.video_path_label.pack(side='left', padx=5, pady=5)
        
        # --- Conversion Options ---
        options_frame = ttk.LabelFrame(frame, text="2. Set Options")
        options_frame.pack(padx=10, pady=10, fill='x')

        # Resolution
        ttk.Label(options_frame, text="Output Width (chars):").grid(row=0, column=0, padx=5, pady=5, sticky='w')
        self.output_width_var = tk.IntVar(value=120)
        ttk.Spinbox(options_frame, from_=40, to=500, textvariable=self.output_width_var, width=10).grid(row=0, column=1, padx=5, pady=5, sticky='w')
        
        # Character Set
        ttk.Label(options_frame, text="Character Set:").grid(row=1, column=0, padx=5, pady=5, sticky='w')
        self.charset_var = tk.StringVar(value="Standard")
        charset_menu = ttk.Combobox(options_frame, textvariable=self.charset_var, values=list(CHAR_SETS.keys()), state="readonly")
        charset_menu.grid(row=1, column=1, padx=5, pady=5, sticky='w')

        # --- Conversion Control ---
        convert_frame = ttk.LabelFrame(frame, text="3. Convert")
        convert_frame.pack(padx=10, pady=10, fill='x')

        self.convert_btn = ttk.Button(convert_frame, text="Start Conversion", command=self._start_conversion)
        self.convert_btn.pack(pady=10)

        # --- Progress & Status ---
        status_frame = ttk.LabelFrame(frame, text="Status")
        status_frame.pack(padx=10, pady=10, fill='x', expand=True)
        
        self.progress_bar = ttk.Progressbar(status_frame, orient='horizontal', length=300, mode='determinate')
        self.progress_bar.pack(pady=5, padx=10, fill='x')
        self.status_label = ttk.Label(status_frame, text="Ready.")
        self.status_label.pack(pady=5, padx=10)

    def _create_player_ui(self):
        frame = self.player_frame

        # --- ASCII File Loading ---
        load_frame = ttk.Frame(frame)
        load_frame.pack(fill='x', padx=10, pady=5)
        self.load_btn = ttk.Button(load_frame, text="Load ASCII Video (.ascii_vid)", command=self._load_ascii_file)
        self.load_btn.pack(side='left', pady=5)
        self.ascii_file_label = ttk.Label(load_frame, text="No file loaded.")
        self.ascii_file_label.pack(side='left', padx=10, pady=5)

        # --- Playback Screen ---
        self.ascii_display = tk.Label(frame, text="Load a file to begin playback.", font=self.monospace_font, justify='left', bg='black', fg='white')
        self.ascii_display.pack(pady=5, padx=10, fill='both', expand=True)

        # --- Playback Controls ---
        controls_frame = ttk.Frame(frame)
        controls_frame.pack(fill='x', padx=10, pady=5)

        self.play_pause_btn = ttk.Button(controls_frame, text="▶ Play", command=self._toggle_playback, state='disabled')
        self.play_pause_btn.pack(side='left', padx=5)

        self.playback_slider_var = tk.DoubleVar()
        self.playback_slider = ttk.Scale(controls_frame, from_=0, to=100, orient='horizontal', variable=self.playback_slider_var, command=self._on_slider_move, state='disabled')
        self.playback_slider.pack(side='left', fill='x', expand=True, padx=5)

        ttk.Label(controls_frame, text="FPS:").pack(side='left')
        self.fps_var = tk.IntVar(value=30)
        fps_spinbox = ttk.Spinbox(controls_frame, from_=1, to=120, textvariable=self.fps_var, width=5)
        fps_spinbox.pack(side='left', padx=5)


    # --- Converter Logic ---

    def _select_video_file(self):
        path = filedialog.askopenfilename(
            title="Select a video file",
            filetypes=(("Video Files", "*.mp4 *.avi *.mov *.mkv"), ("All files", "*.*"))
        )
        if path:
            self.video_path = path
            self.video_path_label.config(text=os.path.basename(path))
            self.status_label.config(text=f"Selected: {os.path.basename(path)}")

    def _start_conversion(self):
        if not self.video_path:
            messagebox.showerror("Error", "Please select a video file first.")
            return
        
        # Disable controls during conversion
        self.convert_btn.config(state='disabled')
        self.select_btn.config(state='disabled')
        self.status_label.config(text="Initializing conversion...")
        self.progress_bar['value'] = 0

        # Start the worker thread
        self.worker_thread = threading.Thread(
            target=self._conversion_worker,
            args=(self.video_path, self.output_width_var.get(), self.charset_var.get()),
            daemon=True
        )
        self.worker_thread.start()

    def _conversion_worker(self, video_path, width, charset_name):
        try:
            char_set = CHAR_SETS[charset_name]
            num_chars = len(char_set)
            
            cap = cv2.VideoCapture(video_path)
            total_frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
            original_fps = cap.get(cv2.CAP_PROP_FPS)

            ascii_frames = []

            for i in range(total_frames):
                ret, frame = cap.read()
                if not ret:
                    break

                # Update progress
                progress = (i + 1) / total_frames * 100
                self.progress_queue.put(('progress', progress, f"Processing frame {i+1}/{total_frames}..."))

                # --- Core ASCII Conversion ---
                # 1. Resize and maintain aspect ratio
                h, w, _ = frame.shape
                aspect_ratio = h / w
                new_height = int(width * aspect_ratio * 0.5) # 0.5 correction for char aspect ratio
                resized_frame = cv2.resize(frame, (width, new_height))

                # 2. Convert to grayscale
                gray_frame = cv2.cvtColor(resized_frame, cv2.COLOR_BGR2GRAY)

                # 3. Map pixels to characters
                # Vectorized operation using NumPy for speed
                indices = (gray_frame / 255 * (num_chars - 1)).astype(int)
                char_array = np.array(list(char_set))[indices]
                
                # 4. Join characters to form the frame string
                ascii_frame_str = "\n".join("".join(row) for row in char_array)
                ascii_frames.append(ascii_frame_str)
            
            cap.release()

            # --- Prepare data for saving ---
            output_data = {
                'fps': original_fps,
                'frames': ascii_frames
            }

            # Signal completion
            self.progress_queue.put(('done', output_data, video_path))

        except Exception as e:
            self.progress_queue.put(('error', str(e)))

    def _check_queue(self):
        """ Periodically check the queue for messages from the worker thread. """
        try:
            message_type, data, *extra = self.progress_queue.get_nowait()
            
            if message_type == 'progress':
                self.progress_bar['value'] = data
                self.status_label.config(text=extra[0])
            
            elif message_type == 'done':
                self._save_ascii_file(data, extra[0])
                self.status_label.config(text="Conversion complete! File saved.")
                self.progress_bar['value'] = 100
                self.convert_btn.config(state='normal')
                self.select_btn.config(state='normal')

            elif message_type == 'error':
                messagebox.showerror("Conversion Error", f"An error occurred: {data}")
                self.status_label.config(text="Error during conversion.")
                self.convert_btn.config(state='normal')
                self.select_btn.config(state='normal')

        except queue.Empty:
            pass # No messages
        
        # Reschedule the check
        self.after(100, self._check_queue)

    def _save_ascii_file(self, data, source_video_path):
        output_filename = os.path.splitext(source_video_path)[0] + ".ascii_vid"
        save_path = filedialog.asksaveasfilename(
            initialfile=os.path.basename(output_filename),
            defaultextension=".ascii_vid",
            filetypes=[("ASCII Video Files", "*.ascii_vid")]
        )
        if save_path:
            with open(save_path, 'wb') as f:
                pickle.dump(data, f)
            self.status_label.config(text=f"File saved to {os.path.basename(save_path)}")

    # --- Player Logic ---

    def _load_ascii_file(self):
        path = filedialog.askopenfilename(
            title="Select an ASCII video file",
            filetypes=(("ASCII Video Files", "*.ascii_vid"),)
        )
        if not path:
            return
        
        try:
            with open(path, 'rb') as f:
                self.ascii_data = pickle.load(f)

            # Validate loaded data
            if 'fps' not in self.ascii_data or 'frames' not in self.ascii_data:
                raise ValueError("Invalid file format.")

            self.ascii_file_label.config(text=os.path.basename(path))
            
            # Reset player state
            self._stop_playback()
            self.current_frame_index = 0
            
            # Setup controls
            num_frames = len(self.ascii_data['frames'])
            self.playback_slider.config(to=num_frames - 1, state='normal')
            self.playback_slider_var.set(0)
            self.play_pause_btn.config(state='normal')
            self.fps_var.set(int(self.ascii_data['fps']))

            # Display first frame
            self.ascii_display.config(text=self.ascii_data['frames'][0])

        except Exception as e:
            messagebox.showerror("Error Loading File", f"Could not load or parse the file:\n{e}")
            self.ascii_data = None


    def _toggle_playback(self):
        if self.ascii_data is None:
            return
            
        self.playback_paused = not self.playback_paused
        if self.playback_paused:
            self.play_pause_btn.config(text="▶ Play")
            if self.playback_job:
                self.after_cancel(self.playback_job)
                self.playback_job = None
        else:
            self.play_pause_btn.config(text="❚❚ Pause")
            # If at the end, restart from the beginning
            if self.current_frame_index >= len(self.ascii_data['frames']) - 1:
                self.current_frame_index = 0
            self._playback_loop()

    def _stop_playback(self):
        if self.playback_job:
            self.after_cancel(self.playback_job)
            self.playback_job = None
        self.playback_paused = True
        self.play_pause_btn.config(text="▶ Play")

    def _playback_loop(self):
        if self.playback_paused or self.ascii_data is None:
            return
        
        num_frames = len(self.ascii_data['frames'])
        if self.current_frame_index < num_frames:
            # Update display and slider
            frame_content = self.ascii_data['frames'][self.current_frame_index]
            self.ascii_display.config(text=frame_content)
            self.playback_slider_var.set(self.current_frame_index)
            
            self.current_frame_index += 1
            
            # Schedule next frame
            delay_ms = int(1000 / self.fps_var.get())
            self.playback_job = self.after(delay_ms, self._playback_loop)
        else:
            # Reached end of video
            self._stop_playback()
            self.current_frame_index = 0
            self.playback_slider_var.set(0)


    def _on_slider_move(self, value_str):
        if self.ascii_data:
            new_index = int(float(value_str))
            if self.current_frame_index != new_index:
                self.current_frame_index = new_index
                # Display the frame for the new position
                frame_content = self.ascii_data['frames'][self.current_frame_index]
                self.ascii_display.config(text=frame_content)

if __name__ == "__main__":
    app = AsciiVideoPlayer()
    app.mainloop()