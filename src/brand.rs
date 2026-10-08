//! Shared Snap Notes mark: sticky note with a strip bar on the right.

/// Warm paper fill used on color surfaces (Windows tray, window, `.exe`).
pub const PAPER: [u8; 3] = [232, 168, 124];
/// Adhesive band across the top of the sticky.
pub const BAND: [u8; 3] = [214, 140, 96];
/// Ink for the strip bar and hairlines on color icons.
pub const INK: [u8; 3] = [48, 44, 40];

/// Coloured brand mark as premultiplied-ready RGBA (opaque pixels).
pub fn icon_rgba(size: u32) -> Vec<u8> {
    paint(size, false)
}

/// Monochrome template mark for macOS menu bar (black on transparent).
#[cfg(any(target_os = "macos", test))]
pub fn template_rgba(size: u32) -> Vec<u8> {
    paint(size, true)
}

fn paint(size: u32, template: bool) -> Vec<u8> {
    let s = size as f32 / 32.0;
    let mut rgba = vec![0; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let in_rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
                fx >= x0 * s && fx < x1 * s && fy >= y0 * s && fy < y1 * s
            };
            // Sticky body and rounded-ish corners by skipping the far corners.
            let body = in_rect(6.0, 5.0, 24.0, 27.0);
            let corner = [(6.0, 5.0), (23.0, 5.0), (6.0, 26.0), (23.0, 26.0)]
                .iter()
                .any(|&(cx, cy)| in_rect(cx, cy, cx + 1.0, cy + 1.0));
            let band = in_rect(6.0, 5.0, 24.0, 9.0) && !corner;
            let bar = in_rect(25.0, 8.0, 28.0, 24.0);
            let line = [12.0, 16.0, 20.0]
                .iter()
                .any(|&ly| in_rect(9.0, ly, 20.0, ly + 1.5));
            let i = ((y * size + x) * 4) as usize;
            if template {
                if (body && !corner) || bar {
                    // Hollow sticky: outline + bar + text lines.
                    let hollow = in_rect(8.0, 9.0, 22.0, 25.0);
                    if (body && !hollow && !corner) || bar || (line && hollow) {
                        rgba[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
                    }
                }
            } else if bar {
                rgba[i..i + 4].copy_from_slice(&[INK[0], INK[1], INK[2], 255]);
            } else if body && !corner {
                let c = if band { BAND } else { PAPER };
                rgba[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                if line && !band {
                    rgba[i..i + 4].copy_from_slice(&[INK[0], INK[1], INK[2], 180]);
                }
            }
        }
    }
    rgba
}

/// A Windows `.ico` with 16, 32 and 48 px colour frames.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ico_bytes() -> Vec<u8> {
    let sizes = [16_u32, 32, 48];
    let images: Vec<(u32, Vec<u8>)> = sizes
        .iter()
        .map(|&s| (s, dib_bgra(s, &icon_rgba(s))))
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // icon
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let header = 6 + 16 * images.len();
    let mut offset = header;
    for (size, dib) in &images {
        out.push(*size as u8); // width
        out.push(*size as u8); // height
        out.push(0); // colours
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bit count
        out.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += dib.len();
    }
    for (_, dib) in images {
        out.extend_from_slice(&dib);
    }
    out
}

/// BITMAPINFOHEADER + BGRA pixels (bottom-up) + empty AND mask, for an ICO image.
#[cfg_attr(not(test), allow(dead_code))]
fn dib_bgra(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut dib = Vec::new();
    dib.extend_from_slice(&40u32.to_le_bytes()); // header size
    dib.extend_from_slice(&(size as i32).to_le_bytes());
    dib.extend_from_slice(&(size as i32 * 2).to_le_bytes()); // height includes mask
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    dib.extend_from_slice(&0u32.to_le_bytes()); // image size
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    // Bottom-up BGRA.
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            dib.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    // AND mask: 1 bit/pixel, padded to 32-bit rows.
    let row = size.div_ceil(32) * 4;
    dib.resize(dib.len() + (row * size) as usize, 0);
    dib
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_icon_has_opaque_pixels() {
        let rgba = icon_rgba(32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert!(rgba.chunks(4).any(|p| p[3] == 255));
        assert!(rgba.chunks(4).any(|p| p[3] == 0));
    }

    #[test]
    fn template_icon_is_black_or_clear() {
        for p in template_rgba(36).chunks(4) {
            assert!(p[3] == 0 || (p[0] == 0 && p[1] == 0 && p[2] == 0 && p[3] == 255));
        }
    }

    #[test]
    fn ico_has_icon_header() {
        let ico = ico_bytes();
        assert_eq!(&ico[0..4], &[0, 0, 1, 0]);
        assert_eq!(ico[4], 3); // three images
        assert!(ico.len() > 1000);
    }

    #[test]
    fn writes_assets_icon_ico() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon.ico");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, ico_bytes()).unwrap();
        assert!(path.metadata().unwrap().len() > 1000);
    }
}
