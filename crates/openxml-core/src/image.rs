//! Recognition of image formats and pixel dimensions from file headers.

use crate::units::Length;

/// Image formats accepted by Office documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    /// Portable Network Graphics.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF.
    Gif,
    /// Windows bitmap.
    Bmp,
    /// TIFF.
    Tiff,
    /// Enhanced metafile.
    Emf,
    /// Windows metafile.
    Wmf,
}

impl ImageFormat {
    /// Conventional file extension.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Gif => "gif",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Emf => "emf",
            ImageFormat::Wmf => "wmf",
        }
    }

    /// Content type of the image part.
    pub fn content_type(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Bmp => "image/bmp",
            ImageFormat::Tiff => "image/tiff",
            ImageFormat::Emf => "image/x-emf",
            ImageFormat::Wmf => "image/x-wmf",
        }
    }
}

/// Format, pixel size and resolution of an image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageInfo {
    /// Format.
    pub format: ImageFormat,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Horizontal resolution in dots per inch.
    pub dpi_x: f64,
    /// Vertical resolution in dots per inch.
    pub dpi_y: f64,
}

impl ImageInfo {
    /// The printed size implied by the pixel size and resolution.
    pub fn natural_size(&self) -> (Length, Length) {
        (
            Length::inches(f64::from(self.width) / self.dpi_x),
            Length::inches(f64::from(self.height) / self.dpi_y),
        )
    }

    /// Scales the natural size to `width`, keeping the aspect ratio.
    pub fn size_for_width(&self, width: Length) -> (Length, Length) {
        if self.width == 0 {
            return (width, width);
        }
        let height = width * (f64::from(self.height) / f64::from(self.width));
        (width, height)
    }
}

const DEFAULT_DPI: f64 = 96.0;

fn be16(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?])))
}
fn le16(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?])))
}
fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

fn png(b: &[u8]) -> Option<ImageInfo> {
    let width = be32(b, 16)?;
    let height = be32(b, 20)?;
    let (mut dpi_x, mut dpi_y) = (DEFAULT_DPI, DEFAULT_DPI);
    let mut pos = 8;
    while pos + 8 <= b.len() {
        let len = be32(b, pos)? as usize;
        let kind = b.get(pos + 4..pos + 8)?;
        if kind == b"pHYs" && len >= 9 && b.get(pos + 16) == Some(&1) {
            dpi_x = f64::from(be32(b, pos + 8)?) * 0.0254;
            dpi_y = f64::from(be32(b, pos + 12)?) * 0.0254;
        }
        if kind == b"IDAT" || kind == b"IEND" {
            break;
        }
        pos += 12 + len;
    }
    Some(ImageInfo {
        format: ImageFormat::Png,
        width,
        height,
        dpi_x,
        dpi_y,
    })
}

fn jpeg(b: &[u8]) -> Option<ImageInfo> {
    let (mut dpi_x, mut dpi_y) = (DEFAULT_DPI, DEFAULT_DPI);
    let mut pos = 2;
    while pos + 4 <= b.len() {
        if b[pos] != 0xFF {
            pos += 1;
            continue;
        }
        let marker = b[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            pos += 2;
            continue;
        }
        let len = be16(b, pos + 2)? as usize;
        let seg = pos + 4;
        if marker == 0xE0 && b.get(seg..seg + 5) == Some(b"JFIF\0") {
            let units = *b.get(seg + 7)?;
            let (x, y) = (f64::from(be16(b, seg + 8)?), f64::from(be16(b, seg + 10)?));
            if x > 0.0 && y > 0.0 {
                match units {
                    1 => (dpi_x, dpi_y) = (x, y),
                    2 => (dpi_x, dpi_y) = (x * 2.54, y * 2.54),
                    _ => {}
                }
            }
        }
        let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let height = be16(b, seg + 1)?;
            let width = be16(b, seg + 3)?;
            return Some(ImageInfo {
                format: ImageFormat::Jpeg,
                width,
                height,
                dpi_x,
                dpi_y,
            });
        }
        if marker == 0xDA {
            break;
        }
        pos += 2 + len;
    }
    None
}

fn bmp(b: &[u8]) -> Option<ImageInfo> {
    let width = le32(b, 18)? as i32;
    let height = le32(b, 22)? as i32;
    let ppm_x = le32(b, 38).unwrap_or(0);
    let ppm_y = le32(b, 42).unwrap_or(0);
    let dpi = |ppm: u32| {
        if ppm > 0 {
            f64::from(ppm) * 0.0254
        } else {
            DEFAULT_DPI
        }
    };
    Some(ImageInfo {
        format: ImageFormat::Bmp,
        width: width.unsigned_abs(),
        height: height.unsigned_abs(),
        dpi_x: dpi(ppm_x),
        dpi_y: dpi(ppm_y),
    })
}

fn tiff(b: &[u8]) -> Option<ImageInfo> {
    let le = b.starts_with(b"II");
    let r16 = |i| if le { le16(b, i) } else { be16(b, i) };
    let r32 = |i| if le { le32(b, i) } else { be32(b, i) };
    let ifd = r32(4)? as usize;
    let count = r16(ifd)? as usize;
    let (mut width, mut height) = (None, None);
    for n in 0..count {
        let e = ifd + 2 + n * 12;
        let tag = r16(e)?;
        let ty = r16(e + 2)?;
        let value = if ty == 3 { r16(e + 8)? } else { r32(e + 8)? };
        match tag {
            256 => width = Some(value),
            257 => height = Some(value),
            _ => {}
        }
    }
    Some(ImageInfo {
        format: ImageFormat::Tiff,
        width: width?,
        height: height?,
        dpi_x: DEFAULT_DPI,
        dpi_y: DEFAULT_DPI,
    })
}

fn emf(b: &[u8]) -> Option<ImageInfo> {
    // EMR_HEADER: rclFrame (in 0.01 mm) at offset 24..40.
    let frame = |i| le32(b, i).map(|v| v as i32);
    let (l, t, r, btm) = (frame(24)?, frame(28)?, frame(32)?, frame(36)?);
    let width_mm = f64::from(r - l) / 100.0;
    let height_mm = f64::from(btm - t) / 100.0;
    Some(ImageInfo {
        format: ImageFormat::Emf,
        width: (width_mm / 25.4 * DEFAULT_DPI).round().max(0.0) as u32,
        height: (height_mm / 25.4 * DEFAULT_DPI).round().max(0.0) as u32,
        dpi_x: DEFAULT_DPI,
        dpi_y: DEFAULT_DPI,
    })
}

fn wmf(b: &[u8]) -> Option<ImageInfo> {
    // Placeable header: bounding box at 6..14 in logical units, units per inch at 14.
    let s = |i| le16(b, i).map(|v| v as i16);
    let (l, t, r, btm) = (s(6)?, s(8)?, s(10)?, s(12)?);
    let per_inch = f64::from(le16(b, 14)?.max(1));
    Some(ImageInfo {
        format: ImageFormat::Wmf,
        width: (f64::from(r - l) / per_inch * DEFAULT_DPI).round().max(0.0) as u32,
        height: (f64::from(btm - t) / per_inch * DEFAULT_DPI).round().max(0.0) as u32,
        dpi_x: DEFAULT_DPI,
        dpi_y: DEFAULT_DPI,
    })
}

/// Detects the format and size of an image from its bytes.
pub fn sniff_image(bytes: &[u8]) -> Option<ImageInfo> {
    let b = bytes;
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(b)
    } else if b.starts_with(&[0xFF, 0xD8]) {
        jpeg(b)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(ImageInfo {
            format: ImageFormat::Gif,
            width: le16(b, 6)?,
            height: le16(b, 8)?,
            dpi_x: DEFAULT_DPI,
            dpi_y: DEFAULT_DPI,
        })
    } else if b.starts_with(b"BM") {
        bmp(b)
    } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        tiff(b)
    } else if le32(b, 0) == Some(1) && b.get(40..44) == Some(b" EMF") {
        emf(b)
    } else if le32(b, 0) == Some(0x9AC6_CDD7) {
        wmf(b)
    } else {
        None
    }
}

/// A minimal valid PNG of the given size (single-colour, 8-bit greyscale), for tests and placeholders.
pub fn tiny_png(width: u32, height: u32) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &d in data {
            a = (a + u32::from(d)) % 65_521;
            b = (b + a) % 65_521;
        }
        (b << 16) | a
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    // Raw scanlines (filter byte 0 + grey pixels), stored in uncompressed deflate blocks.
    let mut raw = Vec::new();
    for _ in 0..height {
        raw.push(0);
        raw.extend(std::iter::repeat_n(0x80u8, width as usize));
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if raw.is_empty() {
        vec![&[][..]]
    } else {
        raw.chunks(65_535).collect()
    };
    for (i, block) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_size_and_default_resolution() {
        let info = sniff_image(&tiny_png(30, 20)).unwrap();
        assert_eq!(info.format, ImageFormat::Png);
        assert_eq!((info.width, info.height), (30, 20));
        assert_eq!(info.dpi_x, 96.0);
        let (w, h) = info.natural_size();
        assert_eq!((w.as_px().round(), h.as_px().round()), (30.0, 20.0));
        let (w2, h2) = info.size_for_width(Length::px(60.0));
        assert_eq!((w2.as_px().round(), h2.as_px().round()), (60.0, 40.0));
    }

    #[test]
    fn png_physical_resolution() {
        let mut png = tiny_png(10, 10);
        // Insert a pHYs chunk (2835 px/m ≈ 72 dpi) after IHDR.
        let mut phys = Vec::new();
        phys.extend_from_slice(&9u32.to_be_bytes());
        phys.extend_from_slice(b"pHYs");
        phys.extend_from_slice(&2835u32.to_be_bytes());
        phys.extend_from_slice(&2835u32.to_be_bytes());
        phys.push(1);
        phys.extend_from_slice(&[0, 0, 0, 0]);
        png.splice(33..33, phys);
        let info = sniff_image(&png).unwrap();
        assert!((info.dpi_x - 72.0).abs() < 0.1, "{}", info.dpi_x);
    }

    #[test]
    fn jpeg_gif_bmp_tiff_headers() {
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 1, 0, 150, 0, 150, 0, 0,
            0xFF, 0xC0, 0x00, 0x11, 8, 0x01, 0x00, 0x02, 0x00, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1,
        ];
        let info = sniff_image(&jpeg).unwrap();
        assert_eq!(
            (info.format, info.width, info.height),
            (ImageFormat::Jpeg, 512, 256)
        );
        assert_eq!(info.dpi_x, 150.0);

        let gif = *b"GIF89a\x40\x00\x20\x00\x00\x00\x00";
        let info = sniff_image(&gif).unwrap();
        assert_eq!((info.format, info.width, info.height), (ImageFormat::Gif, 64, 32));

        let mut bmp = vec![0u8; 54];
        bmp[..2].copy_from_slice(b"BM");
        bmp[18..22].copy_from_slice(&100i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-50i32).to_le_bytes());
        let info = sniff_image(&bmp).unwrap();
        assert_eq!(
            (info.format, info.width, info.height),
            (ImageFormat::Bmp, 100, 50)
        );

        let mut tif = b"II*\0\x08\0\0\0".to_vec();
        tif.extend_from_slice(&2u16.to_le_bytes());
        for (tag, v) in [(256u16, 7u32), (257, 9)] {
            tif.extend_from_slice(&tag.to_le_bytes());
            tif.extend_from_slice(&4u16.to_le_bytes());
            tif.extend_from_slice(&1u32.to_le_bytes());
            tif.extend_from_slice(&v.to_le_bytes());
        }
        let info = sniff_image(&tif).unwrap();
        assert_eq!((info.format, info.width, info.height), (ImageFormat::Tiff, 7, 9));
    }

    #[test]
    fn metafiles() {
        let mut emf = vec![0u8; 88];
        emf[0..4].copy_from_slice(&1u32.to_le_bytes());
        emf[32..36].copy_from_slice(&2540i32.to_le_bytes()); // 25.4 mm wide
        emf[36..40].copy_from_slice(&1270i32.to_le_bytes());
        emf[40..44].copy_from_slice(b" EMF");
        let info = sniff_image(&emf).unwrap();
        assert_eq!((info.format, info.width, info.height), (ImageFormat::Emf, 96, 48));

        let mut wmf = vec![0u8; 22];
        wmf[0..4].copy_from_slice(&0x9AC6_CDD7u32.to_le_bytes());
        wmf[10..12].copy_from_slice(&1440i16.to_le_bytes());
        wmf[12..14].copy_from_slice(&720i16.to_le_bytes());
        wmf[14..16].copy_from_slice(&1440u16.to_le_bytes());
        let info = sniff_image(&wmf).unwrap();
        assert_eq!((info.format, info.width, info.height), (ImageFormat::Wmf, 96, 48));
    }

    #[test]
    fn unknown_and_truncated_data() {
        assert!(sniff_image(b"hello").is_none());
        assert!(sniff_image(b"\x89PNG\r\n\x1a\n").is_none());
        assert!(sniff_image(&[0xFF, 0xD8, 0xFF]).is_none());
        assert!(sniff_image(b"GIF89a").is_none());
    }

    #[test]
    fn format_metadata() {
        for f in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::Bmp,
            ImageFormat::Tiff,
            ImageFormat::Emf,
            ImageFormat::Wmf,
        ] {
            assert!(f.content_type().starts_with("image/"));
            assert!(!f.extension().is_empty());
        }
    }
}
