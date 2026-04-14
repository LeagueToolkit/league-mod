//! Thumbnail loading and WebP conversion.

use super::PackError;
use camino::Utf8Path;
use image::ImageFormat;
use std::fs::{self, File};
use std::io::{BufReader, Cursor};

/// Maximum thumbnail file size: 5MB
pub const MAX_THUMBNAIL_SIZE: u64 = 5 * 1024 * 1024;

/// Load and convert a thumbnail image to WebP format.
///
/// Supports all common image formats (PNG, JPEG, GIF, BMP, TIFF, ICO, WebP).
/// Animated GIFs are converted to animated WebP.
/// Validates file size (max 5MB).
pub fn load_thumbnail(path: &Utf8Path) -> Result<Vec<u8>, PackError> {
    let metadata = fs::metadata(path).map_err(PackError::Io)?;
    if metadata.len() > MAX_THUMBNAIL_SIZE {
        return Err(PackError::ThumbnailError(format!(
            "Thumbnail file size ({} bytes) exceeds maximum allowed size ({} bytes / 5MB)",
            metadata.len(),
            MAX_THUMBNAIL_SIZE
        )));
    }

    let extension = path
        .extension()
        .map(|ext| ext.to_lowercase())
        .unwrap_or_default();

    if extension == "webp" {
        let data = fs::read(path).map_err(PackError::Io)?;
        if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
            return Ok(data);
        }
        return Err(PackError::ThumbnailError(
            "Invalid WebP file format".to_string(),
        ));
    }

    if extension == "gif" {
        return convert_gif_to_webp(path);
    }

    let img = image::open(path)
        .map_err(|e| PackError::ThumbnailError(format!("Failed to open image: {}", e)))?;

    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, ImageFormat::WebP)
        .map_err(|e| PackError::ThumbnailError(format!("Failed to convert to WebP: {}", e)))?;

    Ok(buffer.into_inner())
}

fn convert_gif_to_webp(path: &Utf8Path) -> Result<Vec<u8>, PackError> {
    let file = File::open(path).map_err(PackError::Io)?;
    let reader = BufReader::new(file);
    let decoder = image::codecs::gif::GifDecoder::new(reader)
        .map_err(|e| PackError::ThumbnailError(format!("Failed to decode GIF: {}", e)))?;

    let frames: Vec<_> = image::AnimationDecoder::into_frames(decoder)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| PackError::ThumbnailError(format!("Failed to read GIF frames: {}", e)))?;

    if frames.is_empty() {
        return Err(PackError::ThumbnailError("GIF has no frames".to_string()));
    }

    if frames.len() == 1 {
        let frame = &frames[0];
        let img = frame.buffer();
        let mut buffer = Cursor::new(Vec::new());
        img.write_to(&mut buffer, ImageFormat::WebP).map_err(|e| {
            PackError::ThumbnailError(format!("Failed to convert GIF to WebP: {}", e))
        })?;
        return Ok(buffer.into_inner());
    }

    encode_animated_webp(&frames)
}

fn encode_animated_webp(frames: &[image::Frame]) -> Result<Vec<u8>, PackError> {
    use webp_animation::prelude::*;

    if frames.is_empty() {
        return Err(PackError::ThumbnailError("No frames to encode".to_string()));
    }

    let first_frame = frames[0].buffer();
    let (width, height) = first_frame.dimensions();

    let mut encoder = Encoder::new((width, height)).map_err(|e| {
        PackError::ThumbnailError(format!("Failed to create WebP encoder: {:?}", e))
    })?;

    let mut timestamp_ms = 0i32;
    for frame in frames {
        let img_buffer = frame.buffer();
        let delay = frame.delay();
        let rgba_data = img_buffer.as_raw();

        encoder
            .add_frame(rgba_data, timestamp_ms)
            .map_err(|e| PackError::ThumbnailError(format!("Failed to add frame: {:?}", e)))?;

        let delay_ms = delay.numer_denom_ms();
        timestamp_ms += delay_ms.0 as i32;
    }

    let webp_data = encoder
        .finalize(timestamp_ms)
        .map_err(|e| PackError::ThumbnailError(format!("Failed to finalize animation: {:?}", e)))?;

    Ok(webp_data.to_vec())
}
