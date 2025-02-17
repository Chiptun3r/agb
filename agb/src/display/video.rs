use super::{bitmap3::Bitmap3, tiled::TiledBackground};

/// The video struct controls access to the video hardware.
/// It ensures that only one video mode is active at a time.
///
/// Most games will use tiled modes, as bitmap modes are too slow to run at the full 60 FPS.
#[non_exhaustive]
pub struct Video;

impl Video {
    /// Bitmap mode that provides a 16-bit colour framebuffer
    pub(crate) fn bitmap3(&mut self) -> Bitmap3<'_> {
        unsafe { Bitmap3::new() }
    }

    /// Tiled mode allows for up to 4 backgrounds
    pub fn tiled(&mut self) -> TiledBackground<'_> {
        unsafe { TiledBackground::new() }
    }
}
