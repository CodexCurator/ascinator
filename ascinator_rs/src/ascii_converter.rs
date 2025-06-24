use ffmpeg_next::{format::{input, Pixel}, media::Type, software::scaling::{context::Context, flag::Flags}, util::frame::video::Video as FrameVideo};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn get_char_sets() -> HashMap<String, &'static str> {
    let mut char_sets = HashMap::new();
    char_sets.insert("Standard".to_string(), "@%#*+=-:. ");
    char_sets.insert("Detailed".to_string(), "$@B%8&WM#*oahkbdpqwmZO0QLCJUYXzcvunxrjft/\\|()1{}[]?-_+~<>i!lI;:,\"^`'. ");
    char_sets.insert("Blocks".to_string(), "█▇▆▅▄▃▂  ");
    char_sets.insert("Simple".to_string(), "#=-. ");
    char_sets.insert("Gradient".to_string(), " .:!/r(l1Z4H9W8$@");
    char_sets
}

#[derive(Debug, Clone)]
pub struct ConversionOptions {
    pub width: u32, // Changed to u32 as ffmpeg uses unsigned for width/height
    pub char_set_name: String,
    // pub progress_callback: Option<Box<dyn Fn(f32, String) + Send>>, // For later GUI integration
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AsciiFrameData {
    pub frames: Vec<String>,
    pub fps: f64,
    pub audio_path: Option<PathBuf>,
}

pub fn video_to_ascii_frames(
    video_path: &str,
    options: &ConversionOptions,
) -> Result<AsciiFrameData, String> {
    ffmpeg_next::init().map_err(|e| format!("FFmpeg: Failed to initialize: {}", e))?;

    let char_sets = get_char_sets();
    let char_set_str = char_sets
        .get(&options.char_set_name)
        .ok_or_else(|| format!("Invalid charset name: {}", options.char_set_name))?;
    let char_list: Vec<char> = char_set_str.chars().collect();
    let num_chars = char_list.len();

    if num_chars == 0 {
        return Err("Character set cannot be empty.".to_string());
    }

    let mut ictx = input(&PathBuf::from(video_path))
        .map_err(|e| format!("FFmpeg: Failed to open input file '{}': {}", video_path, e))?;

    let input_stream = ictx
        .streams()
        .best(Type::Video)
        .ok_or_else(|| "FFmpeg: No suitable video stream found.".to_string())?;

    let video_stream_index = input_stream.index();
    let original_fps = input_stream.avg_frame_rate().as_f64().max(1.0); // Use avg_frame_rate, ensure > 0

    let mut decoder = input_stream.codec().decoder().video()
        .map_err(|e| format!("FFmpeg: Failed to get video decoder: {}", e))?;

    decoder.set_threading_config(Default::default()); // Enable multi-threading if available

    let original_width = decoder.width();
    let original_height = decoder.height();
    if original_width == 0 || original_height == 0 {
        return Err("FFmpeg: Video stream has zero width or height.".to_string());
    }

    let aspect_ratio = original_height as f64 / original_width as f64;
    // 0.5 correction for char aspect ratio (height/width of typical console chars)
    let new_height = (options.width as f64 * aspect_ratio * 0.5).round() as u32;
    if new_height == 0 {
        return Err(format!("Calculated new height is zero for width {}. Original aspect ratio: {}", options.width, aspect_ratio));
    }


    let mut scaler = Context::get(
        decoder.format(),
        original_width,
        original_height,
        Pixel::GRAY8, // Target grayscale
        options.width,
        new_height,
        Flags::BILINEAR, // Or other scaling algorithm
    )
    .map_err(|e| format!("FFmpeg: Failed to create scaling context: {}", e))?;

    let mut ascii_frames: Vec<String> = Vec::new();
    // let total_frames_approx = ictx.duration() as f64 * original_fps / ffmpeg_next::ffi::AV_TIME_BASE as f64; // Approximate

    let mut frame_count = 0;

    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            decoder.send_packet(&packet).map_err(|e| format!("FFmpeg: Failed to send packet to decoder: {}", e))?;
            let mut decoded_frame = FrameVideo::empty();
            while decoder.receive_frame(&mut decoded_frame).is_ok() {
                frame_count += 1;
                // if let Some(cb) = &options.progress_callback {
                //     let progress = if total_frames_approx > 0.0 { (frame_count as f32 / total_frames_approx as f32).min(1.0) * 100.0 } else { 0.0 };
                //     cb(progress, format!("Processing frame {}...", frame_count));
                // }

                let mut scaled_gray_frame = FrameVideo::empty();
                scaler.run(&decoded_frame, &mut scaled_gray_frame)
                    .map_err(|e| format!("FFmpeg: Failed to scale/convert frame {}: {}", frame_count, e))?;

                let mut ascii_frame_str_builder = String::with_capacity((options.width * new_height + new_height) as usize);
                let data = scaled_gray_frame.data(0); // GRAY8 has one plane
                let stride = scaled_gray_frame.stride(0) as usize;

                for r in 0..new_height as usize {
                    for c in 0..options.width as usize {
                        let pixel_val = data[r * stride + c];
                        let char_index = (pixel_val as f32 / 255.0 * (num_chars - 1) as f32).round() as usize;
                        let char_index = char_index.min(num_chars - 1);
                        ascii_frame_str_builder.push(char_list[char_index]);
                    }
                    if r < (new_height as usize) -1 {
                         ascii_frame_str_builder.push('\n');
                    }
                }
                ascii_frames.push(ascii_frame_str_builder);
            }
        }
    }

    // Send EOF to decoder
    decoder.send_eof().map_err(|e| format!("FFmpeg: Failed to send EOF to decoder: {}", e))?;
    let mut decoded_frame = FrameVideo::empty();
    while decoder.receive_frame(&mut decoded_frame).is_ok() {
        // Process any remaining frames flushed from the decoder
        frame_count += 1;
        let mut scaled_gray_frame = FrameVideo::empty();
        scaler.run(&decoded_frame, &mut scaled_gray_frame)
            .map_err(|e| format!("FFmpeg: Failed to scale/convert final frame {}: {}", frame_count, e))?;

        let mut ascii_frame_str_builder = String::with_capacity((options.width * new_height + new_height) as usize);
        let data = scaled_gray_frame.data(0);
        let stride = scaled_gray_frame.stride(0) as usize;

        for r in 0..new_height as usize {
            for c in 0..options.width as usize {
                let pixel_val = data[r * stride + c];
                let char_index = (pixel_val as f32 / 255.0 * (num_chars - 1) as f32).round() as usize;
                ascii_frame_str_builder.push(char_list[char_index.min(num_chars - 1)]);
            }
            if r < (new_height as usize) -1 {
                 ascii_frame_str_builder.push('\n');
            }
        }
        ascii_frames.push(ascii_frame_str_builder);
    }


    if ascii_frames.is_empty() && frame_count == 0 {
        // This can happen if the video is extremely short or only contains non-video streams.
        // Or if no packets were successfully decoded into frames.
        return Err(format!("FFmpeg: No video frames processed from '{}'. Ensure it's a valid video file.", video_path));
    }

    Ok(AsciiFrameData {
        frames: ascii_frames,
        fps: original_fps,
        audio_path: None, // This will be set by the calling code in app_ui.rs after audio extraction
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_char_sets() {
        let sets = get_char_sets();
        assert!(sets.contains_key("Standard"));
        assert_eq!(sets.get("Standard").unwrap(), &"@%#*+=-:. ");
    }

    // To run a meaningful conversion test, we'd need a video file
    // and FFmpeg libraries correctly set up in the test environment.
    // This is a structural placeholder.
    // #[test]
    // fn test_conversion_placeholder_ffmpeg() {
    //     // Ensure FFmpeg is initialized for tests if needed, or mock.
    //     // ffmpeg_next::init().unwrap();
    //     let options = ConversionOptions {
    //         width: 80,
    //         char_set_name: "Simple".to_string(),
    //     };
    //     // Create a dummy video file for testing or use a known small one.
    //     // For CI, this often involves embedding a small video or downloading one.
    //     // let result = video_to_ascii_frames("path/to/test_video.mp4", &options);
    //     // assert!(result.is_ok(), "Conversion failed: {:?}", result.err());
    //     // if let Ok(data) = result {
    //     //     assert!(!data.frames.is_empty());
    //     //     assert!(data.fps > 0.0);
    //     // }
    // }
}
