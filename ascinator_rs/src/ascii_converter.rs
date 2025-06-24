use opencv::{core, prelude::*, videoio, imgproc, Result};
use std::collections::HashMap;

pub fn get_char_sets() -> HashMap<String, &'static str> {
    let mut char_sets = HashMap::new();
    char_sets.insert("Standard".to_string(), "@%#*+=-:. ");
    char_sets.insert("Detailed".to_string(), "$@B%8&WM#*oahkbdpqwmZO0QLCJUYXzcvunxrjft/\\|()1{}[]?-_+~<>i!lI;:,\"^`'. ");
    char_sets.insert("Blocks".to_string(), "█▇▆▅▄▃▂  ");
    char_sets.insert("Simple".to_string(), "#=-. ");
    char_sets.insert("Gradient".to_string(), " .:!/r(l1Z4H9W8$@"); // Already inverted in Python
    char_sets
}

pub struct ConversionOptions {
    pub width: i32,
    pub char_set_name: String,
}

use serde::{Serialize, Deserialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)] // Added derive for Serde and Clone
pub struct AsciiFrameData {
    pub frames: Vec<String>,
    pub fps: f64,
    pub audio_path: Option<PathBuf>, // Store path to the extracted audio file
}

// Placeholder for progress reporting callback
// pub type ProgressCallback = Box<dyn Fn(f32, String) + Send>;

pub fn video_to_ascii_frames(
    video_path: &str,
    options: &ConversionOptions,
    // progress_callback: Option<ProgressCallback>, // Will integrate later with GUI
) -> Result<AsciiFrameData, String> {
    let char_sets = get_char_sets();
    let char_set_str = char_sets.get(&options.char_set_name)
        .ok_or_else(|| format!("Invalid charset name: {}", options.char_set_name))?;
    let char_list: Vec<char> = char_set_str.chars().collect();
    let num_chars = char_list.len();

    if num_chars == 0 {
        return Err("Character set cannot be empty.".to_string());
    }

    let mut cap = videoio::VideoCapture::from_file(video_path, videoio::CAP_ANY)
        .map_err(|e| format!("OpenCV: Failed to open video file '{}': {}", video_path, e))?;

    let total_frames = cap.get(videoio::CAP_PROP_FRAME_COUNT)
        .map_err(|e| format!("OpenCV: Failed to get total frame count for '{}': {}", video_path, e))? as i64;
    let original_fps = cap.get(videoio::CAP_PROP_FPS)
        .map_err(|e| format!("OpenCV: Failed to get FPS for '{}': {}", video_path, e))?;

    if !cap.is_opened().map_err(|e| format!("OpenCV: Capture check failed for '{}': {}", video_path, e))? {
        return Err(format!("OpenCV: Could not open video capture for '{}'. Check path and OpenCV backend/permissions.", video_path));
    }

    let mut ascii_frames: Vec<String> = Vec::with_capacity(total_frames.max(0) as usize); // Pre-allocate
    let mut frame = core::Mat::default();

    for i in 0..total_frames {
        if !cap.read(&mut frame).map_err(|e| format!("Failed to read frame: {}", e))? {
            // End of video or error
            eprintln!("Warning: Could not read frame {} or end of video reached.", i);
            break;
        }
        if frame.empty() {
            eprintln!("Warning: Frame {} is empty.", i);
            continue;
        }

        // --- Core ASCII Conversion ---
        // 1. Resize and maintain aspect ratio
        let h = frame.rows();
        let w = frame.cols();
        if w == 0 || h == 0 {
            eprintln!("Warning: Frame {} has zero width or height.", i);
            continue;
        }

        let aspect_ratio = h as f64 / w as f64;
        // 0.5 correction for char aspect ratio (height/width of typical console chars)
        // This might need adjustment based on font in the display
        let new_height = (options.width as f64 * aspect_ratio * 0.5).round() as i32;
        if new_height <= 0 {
            eprintln!("Warning: Calculated new height is <=0 for frame {}. Skipping.", i);
            continue;
        }


        let mut resized_frame = core::Mat::default();
        imgproc::resize(
            &frame,
            &mut resized_frame,
            core::Size::new(options.width, new_height),
            0.0,
            0.0,
            imgproc::INTER_LINEAR,
        ).map_err(|e| format!("OpenCV: Failed to resize frame {}: {}", i, e))?;

        // 2. Convert to grayscale
        let mut gray_frame = core::Mat::default();
        imgproc::cvt_color(&resized_frame, &mut gray_frame, imgproc::COLOR_BGR2GRAY, 0)
            .map_err(|e| format!("OpenCV: Failed to convert frame {} to grayscale: {}", i, e))?;

        // 3. Map pixels to characters
        let mut ascii_frame_str_builder = String::with_capacity( (options.width * new_height) as usize + new_height as usize); // Pre-allocate for characters + newlines
        for r in 0..gray_frame.rows() {
            for c in 0..gray_frame.cols() {
                let pixel_val = *gray_frame.at_2d::<u8>(r, c)
                    .map_err(|e| format!("OpenCV: Failed to get pixel value at ({}, {}) for frame {}: {}", r, c, i, e))?;
                // Normalize pixel value to range [0, num_chars - 1]
                let char_index = (pixel_val as f32 / 255.0 * (num_chars - 1) as f32).round() as usize;
                // Ensure index is within bounds (it should be, but good to be safe)
                let char_index = char_index.min(num_chars - 1);
                ascii_frame_str_builder.push(char_list[char_index]);
            }
            if r < gray_frame.rows() -1 { // Don't add newline for the last row
                 ascii_frame_str_builder.push('\n');
            }
        }
        ascii_frames.push(ascii_frame_str_builder);

        // if let Some(cb) = &progress_callback {
        //     let progress_val = (i + 1) as f32 / total_frames as f32 * 100.0;
        //     cb(progress_val, format!("Processing frame {}/{}...", i + 1, total_frames));
        // }
    }

    // cap.release().map_err(|e| format!("Failed to release video capture: {}", e))?; // release() is not available on VideoCapture in current opencv rust bindings

    Ok(AsciiFrameData {
        frames: ascii_frames,
        fps: original_fps,
    })
}

// Basic test function (will be expanded in a test module later)
#[cfg(test)]
mod tests {
    use super::*;

    // Note: For proper testing, you'd need a sample video file.
    // This is a placeholder for functionality.
    #[test]
    fn test_get_char_sets() {
        let sets = get_char_sets();
        assert!(sets.contains_key("Standard"));
        assert_eq!(sets.get("Standard").unwrap(), &"@%#*+=-:. ");
    }

    // To run a meaningful conversion test, we'd need a video file
    // and assertions on the output. For now, this is a structural placeholder.
    // #[test]
    // fn test_conversion_placeholder() {
    //     // Create a dummy video file or mock VideoCapture if possible
    //     // For now, this test would likely fail or require a specific setup
    //     let options = ConversionOptions {
    //         width: 80,
    //         char_set_name: "Simple".to_string(),
    //     };
    //     // let result = video_to_ascii_frames("path/to/test_video.mp4", &options);
    //     // assert!(result.is_ok());
    //     // if let Ok(data) = result {
    //     //     assert!(!data.frames.is_empty());
    //     //     assert!(data.fps > 0.0);
    //     // }
    // }
}
