//! Thumbnail loading and WebP conversion.

use camino::Utf8Path;
use image::ImageFormat;
use std::fs::{self, File};
use std::io::{BufReader, Cursor};

/// Maximum thumbnail file size: 5MB
pub const MAX_THUMBNAIL_SIZE: u64 = 5 * 1024 * 1024;

/// Why a thumbnail could not be loaded or converted to WebP.
///
/// Decode and encode failures carry a boxed source: the image codecs this
/// crate uses are an implementation detail, not part of its public API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ThumbnailError {
    #[error("IO error")]
    Io(#[from] std::io::Error),

    #[error("Thumbnail is {size} bytes, which exceeds the {max} byte maximum")]
    TooLarge { size: u64, max: u64 },

    #[error("File has a .webp extension but is not a valid WebP file")]
    InvalidWebp,

    #[error("Image contains no frames")]
    NoFrames,

    #[error("Failed to decode image")]
    Decode(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Failed to encode WebP")]
    Encode(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl ThumbnailError {
    fn decode(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Decode(Box::new(error))
    }

    fn encode(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Encode(Box::new(error))
    }
}

/// Load and convert a thumbnail image to WebP format.
///
/// Supports all common image formats (PNG, JPEG, GIF, BMP, TIFF, ICO, WebP).
/// Animated GIFs are converted to animated WebP.
/// Validates file size (max 5MB).
pub fn load_thumbnail(path: &Utf8Path) -> Result<Vec<u8>, ThumbnailError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_THUMBNAIL_SIZE {
        return Err(ThumbnailError::TooLarge {
            size: metadata.len(),
            max: MAX_THUMBNAIL_SIZE,
        });
    }

    let extension = path
        .extension()
        .map(|ext| ext.to_lowercase())
        .unwrap_or_default();

    if extension == "webp" {
        let data = fs::read(path)?;
        if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
            return Ok(data);
        }
        return Err(ThumbnailError::InvalidWebp);
    }

    if extension == "gif" {
        return convert_gif_to_webp(path);
    }

    let img = image::open(path).map_err(ThumbnailError::decode)?;

    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, ImageFormat::WebP)
        .map_err(ThumbnailError::encode)?;

    Ok(buffer.into_inner())
}

fn convert_gif_to_webp(path: &Utf8Path) -> Result<Vec<u8>, ThumbnailError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let decoder = image::codecs::gif::GifDecoder::new(reader).map_err(ThumbnailError::decode)?;

    let frames: Vec<_> = image::AnimationDecoder::into_frames(decoder)
        .collect::<Result<Vec<_>, _>>()
        .map_err(ThumbnailError::decode)?;

    match frames.as_slice() {
        [] => Err(ThumbnailError::NoFrames),
        [frame] => {
            let mut buffer = Cursor::new(Vec::new());
            frame
                .buffer()
                .write_to(&mut buffer, ImageFormat::WebP)
                .map_err(ThumbnailError::encode)?;
            Ok(buffer.into_inner())
        }
        frames => encode_animated_webp(frames),
    }
}

fn encode_animated_webp(frames: &[image::Frame]) -> Result<Vec<u8>, ThumbnailError> {
    use webp_animation::prelude::*;

    let first_frame = frames.first().ok_or(ThumbnailError::NoFrames)?.buffer();
    let (width, height) = first_frame.dimensions();

    let mut encoder = Encoder::new((width, height)).map_err(ThumbnailError::encode)?;

    let mut timestamp_ms = 0i32;
    for frame in frames {
        encoder
            .add_frame(frame.buffer().as_raw(), timestamp_ms)
            .map_err(ThumbnailError::encode)?;

        timestamp_ms += frame.delay().numer_denom_ms().0 as i32;
    }

    let webp_data = encoder
        .finalize(timestamp_ms)
        .map_err(ThumbnailError::encode)?;

    Ok(webp_data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn write(name: &str, bytes: &[u8]) -> (tempfile::TempDir, Utf8PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(tmp.path().join(name)).unwrap();
        fs::write(&path, bytes).unwrap();
        (tmp, path)
    }

    /// The size limit is the one failure a caller can act on, so it has to be
    /// distinguishable rather than folded into a generic decode failure.
    #[test]
    fn rejects_oversized_thumbnails_with_the_measured_size() {
        let (_tmp, path) = write("big.webp", &vec![0u8; MAX_THUMBNAIL_SIZE as usize + 1]);

        match load_thumbnail(&path) {
            Err(ThumbnailError::TooLarge { size, max }) => {
                assert_eq!(size, MAX_THUMBNAIL_SIZE + 1);
                assert_eq!(max, MAX_THUMBNAIL_SIZE);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[test]
    fn rejects_a_webp_extension_that_is_not_webp() {
        let (_tmp, path) = write("fake.webp", b"definitely not a RIFF container");

        assert!(matches!(
            load_thumbnail(&path),
            Err(ThumbnailError::InvalidWebp)
        ));
    }

    #[test]
    fn passes_through_a_valid_webp_unchanged() {
        let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
        bytes.extend_from_slice(b"payload");
        let (_tmp, path) = write("real.webp", &bytes);

        assert_eq!(load_thumbnail(&path).unwrap(), bytes);
    }

    #[test]
    fn surfaces_a_missing_file_as_io() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(tmp.path().join("missing.png")).unwrap();

        assert!(matches!(load_thumbnail(&path), Err(ThumbnailError::Io(_))));
    }

    #[test]
    fn reports_undecodable_images_as_decode_failures() {
        let (_tmp, path) = write("broken.png", b"not a png");

        assert!(matches!(
            load_thumbnail(&path),
            Err(ThumbnailError::Decode(_))
        ));
    }
}
