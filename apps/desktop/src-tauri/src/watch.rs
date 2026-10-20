//! Watching the desktop for file drags. The signals and the frosted snapshot
//! are shared; noticing drags and reading the screen is per platform
//! (`x11watch` on Linux, `win::watch` on Windows).

use crate::settings::Modifier;

#[cfg(target_os = "linux")]
pub use crate::x11watch::{active_window, pointer_position, screen_size, snapshot, spawn, window_at, WindowInfo};
#[cfg(windows)]
pub use crate::win::watch::{active_window, pointer_position, screen_size, snapshot, spawn, window_at, WindowInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Enter,
    Escape,
    Space,
}

#[derive(Debug, Clone)]
pub enum Signal {
    /// A drag began. `paths` is `Some(vec![])` for drags that aren't files,
    /// and `None` when the source wouldn't tell us what it's dragging.
    DragStarted { paths: Option<Vec<String>> },
    /// A modifier went down mid-drag, with the pointer at this screen position.
    ModifierDown { modifier: Modifier, x: i32, y: i32 },
    Key(Key),
    DragEnded,
}

/// Shrink a screen capture (32 bits per pixel, BGRX, `stride` bytes per row)
/// by `shrink` and return it as a PNG data URL for the wheel's backdrop.
pub fn frost(data: &[u8], w: usize, h: usize, stride: usize, shrink: u32) -> Option<String> {
    use base64::Engine as _;
    if w == 0 || h == 0 || stride < w * 4 || data.len() < stride * (h - 1) + w * 4 {
        return None;
    }
    // Average each `shrink`×`shrink` block straight into the small image; the page blurs it further.
    let k = shrink.max(1) as usize;
    let (sw, sh) = ((w / k).max(1), (h / k).max(1));
    let mut small = ::image::RgbImage::new(sw as u32, sh as u32);
    for sy in 0..sh {
        for sx in 0..sw {
            let mut acc = [0u32; 3];
            for dy in 0..k {
                let row = (sy * k + dy).min(h - 1) * stride;
                for dx in 0..k {
                    let i = row + (sx * k + dx).min(w - 1) * 4;
                    acc[0] += data[i + 2] as u32;
                    acc[1] += data[i + 1] as u32;
                    acc[2] += data[i] as u32;
                }
            }
            let n = (k * k) as u32;
            small.put_pixel(sx as u32, sy as u32, ::image::Rgb([(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8]));
        }
    }
    let mut png = Vec::new();
    small.write_to(&mut std::io::Cursor::new(&mut png), ::image::ImageFormat::Png).ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frosts_bgrx_pixels() {
        // 4×2 BGRX with 4 bytes of row padding: left half red, right half blue.
        let mut data = Vec::new();
        for _ in 0..2 {
            data.extend_from_slice(&[0, 0, 255, 0, 0, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0, 0, 9, 9, 9, 9]);
        }
        let url = frost(&data, 4, 2, 20, 2).unwrap();
        let png = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, url.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
        let img = ::image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (2, 1));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(img.get_pixel(1, 0).0, [0, 0, 255]);
        assert!(frost(&data, 4, 2, 8, 2).is_none());
    }
}
