use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLOrigin, MTLRegion, MTLSize, MTLTexture};
use pathfinder_geometry::vector::Vector2F;
use warpui_core::platform::CapturedFrame;

#[cfg(test)]
#[path = "frame_capture_tests.rs"]
mod tests;

/// Captures a rendered frame from a Metal texture and returns the raw BGRA pixel data.
///
/// The data is returned in Metal's native BGRA format to avoid an expensive
/// pixel-format conversion on the render thread. Consumers that need RGBA
/// should call `CapturedFrame::ensure_rgba()`.
///
/// # Arguments
/// * `texture` - The Metal texture containing the rendered frame
/// * `size` - The dimensions of the texture (width, height)
///
/// # Returns
/// * `Some(CapturedFrame)` containing the RGBA pixel data if successful
/// * `None` if the texture dimensions are invalid
pub fn capture_frame(
    texture: &ProtocolObject<dyn MTLTexture>,
    size: Vector2F,
) -> Option<CapturedFrame> {
    let width = size.x() as usize;
    let height = size.y() as usize;

    if width == 0 || height == 0 {
        log::warn!("Invalid texture dimensions: {width}x{height}");
        return None;
    }

    let bytes_per_row = width * 4;
    let buffer_size = bytes_per_row * height;

    let mut pixel_data: Vec<u8> = vec![0u8; buffer_size];

    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width,
            height,
            depth: 1,
        },
    };

    // SAFETY: `pixel_data` holds `bytes_per_row * height` bytes, matching the requested region and
    // row stride, so Metal copies the texture contents into a valid buffer.
    unsafe {
        texture.getBytes_bytesPerRow_fromRegion_mipmapLevel(
            NonNull::new(pixel_data.as_mut_ptr() as *mut c_void)
                .expect("pixel buffer pointer is non-null"),
            bytes_per_row,
            region,
            0,
        );
    }

    Some(CapturedFrame::new_bgra(
        width as u32,
        height as u32,
        pixel_data,
    ))
}

#[cfg(test)]
pub(crate) fn convert_bgra_to_rgba(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
}
