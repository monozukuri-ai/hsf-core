//! Explicit empirical framing, separate from the published byte-format Image.
//! Encoded pixels stay opaque; JPEG entropy and image placement are not decoded.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageProfile {
    #[default]
    Disabled,
    /// Observed HSF 19.10: u32 flags 0xa3, named local RGBA, baseline JPEG.
    U32RgbaJpegV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageLayout {
    pub profile: String,
    pub position: SourceSpan,
    pub format_options: u32,
    pub name: SourceSpan,
    pub width: u32,
    pub height: u32,
    pub compression: u8,
    pub data: SourceSpan,
    pub alpha_compression: u8,
    /// One constant byte or encoded RLE; never an expanded pixel allocation.
    pub alpha: SourceSpan,
}

fn invalid(message: &str) -> HsfError {
    error("hsf.invalid_image", Status::Invalid, message)
}
fn unsupported(message: &str) -> HsfError {
    error("hsf.image_profile", Status::Unsupported, message)
}

impl Parser<'_> {
    pub(super) fn image(&mut self, d: usize, c: &mut Cursor<'_>) -> Result<ImageLayout, HsfError> {
        if self.options.image_profile == ImageProfile::Disabled {
            // Preserve the default stop and do not reinterpret a modern image
            // through the incompatible published one-byte format field.
            return Err(error(
                "hsf.unknown_opcode",
                Status::Unsupported,
                "Image framing requires an explicit empirical profile",
            ));
        }
        let version = &self.document.header.version;
        if (version.major, version.minor) != (19, 10) {
            return Err(unsupported(
                "The u32 RGBA JPEG profile is limited to HSF 19.10",
            ));
        }
        self.budget.charge_output(1024)?;
        let at = c.pos - 1;
        let position = c.take(d, 12)?;
        progress::finite(&c.data[at + 1..c.pos], "hsf.nonfinite_image")?;
        let format_options = c.u32()?;
        if format_options != 0xa3 {
            return Err(unsupported(
                "Only the observed named local RGBA image flags are supported",
            ));
        }
        let n = c.u8()?;
        if n == 0 {
            return Err(invalid("Named image requires a nonempty name"));
        }
        if u64::from(n) > self.options.max_string_bytes {
            return Err(error(
                "hsf.limit_string",
                Status::LimitExceeded,
                "Image name exceeds string limit",
            ));
        }
        let name = c.take(d, usize::from(n))?;
        let width = c.nonnegative()?;
        let height = c.nonnegative()?;
        if width == 0 || height == 0 {
            return Err(invalid("Image dimensions must be positive"));
        }
        let pixels = u64::from(width) * u64::from(height);
        charge(
            &mut self.image_pixels,
            pixels,
            self.options.max_image_pixels,
            "hsf.limit_image_pixels",
        )?;
        let compression = c.u8()?;
        if compression != 2 {
            return Err(unsupported(
                "Only JPEG image payloads are supported by this profile",
            ));
        }
        let n = c.nonnegative()?;
        let start = c.pos;
        let data = c.array(d, n, 1)?;
        jpeg_envelope(&c.data[start..c.pos], width, height)?;
        let alpha_compression = c.u8()?;
        let alpha = match alpha_compression {
            0 => c.take(d, 1)?,
            1 => {
                let n = c.nonnegative()?;
                let start = c.pos;
                let alpha = c.array(d, n, 1)?;
                alpha_rle(&c.data[start..c.pos], pixels)?;
                alpha
            }
            _ => return Err(unsupported("Unknown JPEG alpha encoding")),
        };
        if self.options.mode == ParseMode::Strict {
            return Err(error(
                "hsf.experimental_image",
                Status::Unsupported,
                "Empirical image framing is forbidden in strict mode",
            ));
        }
        let mut diagnostic = *self
            .fail(
                d,
                at,
                Some(b'i'),
                "hsf.experimental_image",
                Status::Unsupported,
                "Image uses an empirical u32-format profile; pixels and placement are not decoded",
            )
            .diagnostic;
        diagnostic.severity = "warning".into();
        diagnostic.impact =
            Some("Syntax continues with an experimental boundary; result remains partial".into());
        self.diagnostics.push(diagnostic);
        Ok(ImageLayout {
            profile: "u32_rgba_jpeg_v1".into(),
            position,
            format_options,
            name,
            width,
            height,
            compression,
            data,
            alpha_compression,
            alpha,
        })
    }
}

/// Check a bounded single-scan baseline envelope and SOF dimensions. Table
/// contents and entropy codes remain opaque, so this is not JPEG validation.
fn jpeg_envelope(bytes: &[u8], width: u32, height: u32) -> Result<(), HsfError> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(invalid("JPEG SOI is missing"));
    }
    let mut at = 2;
    let mut components: Option<[u8; 3]> = None;
    loop {
        if bytes.get(at) != Some(&0xff) {
            return Err(invalid("JPEG marker expected"));
        }
        while bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *bytes
            .get(at)
            .ok_or_else(|| invalid("Truncated JPEG marker"))?;
        at += 1;
        if !matches!(
            marker,
            0xc0 | 0xc4 | 0xdb | 0xdd | 0xda | 0xe0..=0xef | 0xfe
        ) {
            return Err(unsupported(
                "JPEG envelope is outside the baseline single-scan profile",
            ));
        }
        let size = bytes
            .get(at..at + 2)
            .ok_or_else(|| invalid("Truncated JPEG segment length"))?;
        let size = usize::from(u16::from_be_bytes(size.try_into().unwrap()));
        if size < 2 {
            return Err(invalid("Invalid JPEG segment length"));
        }
        let segment = bytes
            .get(at + 2..at + size)
            .ok_or_else(|| invalid("Truncated JPEG segment"))?;
        at += size;
        if marker == 0xc0 {
            if components.is_some() || segment.len() != 15 || segment[0] != 8 || segment[5] != 3 {
                return Err(unsupported(
                    "JPEG requires one 8-bit three-component baseline frame",
                ));
            }
            let h = u32::from(u16::from_be_bytes([segment[1], segment[2]]));
            let w = u32::from(u16::from_be_bytes([segment[3], segment[4]]));
            if (w, h) != (width, height) {
                return Err(invalid("JPEG and Image dimensions disagree"));
            }
            let ids = [segment[6], segment[9], segment[12]];
            if ids[0] == ids[1] || ids[0] == ids[2] || ids[1] == ids[2] {
                return Err(invalid("Duplicate JPEG component identifiers"));
            }
            components = Some(ids);
        }
        if marker == 0xda {
            let ids = components.ok_or_else(|| invalid("JPEG scan precedes frame"))?;
            if segment.len() != 10 || segment[0] != 3 || segment[7..] != [0, 63, 0] {
                return Err(unsupported(
                    "Only one interleaved baseline JPEG scan is supported",
                ));
            }
            let scan_ids = [segment[1], segment[3], segment[5]];
            if scan_ids.iter().any(|id| !ids.contains(id))
                || scan_ids[0] == scan_ids[1]
                || scan_ids[0] == scan_ids[2]
                || scan_ids[1] == scan_ids[2]
            {
                return Err(invalid("JPEG scan component identifiers disagree"));
            }
            break;
        }
    }
    while at < bytes.len() {
        if bytes[at] != 0xff {
            at += 1;
            continue;
        }
        at += 1;
        while bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *bytes
            .get(at)
            .ok_or_else(|| invalid("Truncated JPEG entropy marker"))?;
        at += 1;
        match marker {
            0 | 0xd0..=0xd7 => (),
            0xd9 if at == bytes.len() => return Ok(()),
            0xd9 => return Err(invalid("Bytes follow JPEG EOI inside declared payload")),
            _ => {
                return Err(unsupported(
                    "Additional JPEG scans or markers are not supported",
                ));
            }
        }
    }
    Err(invalid("JPEG EOI is missing"))
}

/// Count decoded alpha values without allocating or expanding them. A zero
/// control byte repeats the next byte three times; it is not an end marker.
fn alpha_rle(bytes: &[u8], pixels: u64) -> Result<(), HsfError> {
    let mut at = 0;
    let mut count = 0u64;
    while let Some(&control) = bytes.get(at) {
        at += 1;
        let control = i16::from(control as i8);
        let (input, output) = if control > 0 {
            (control as usize, control as u64)
        } else {
            (1, (3 - control) as u64)
        };
        if bytes.get(at..at + input).is_none() {
            return Err(invalid("Truncated alpha RLE run"));
        }
        at += input;
        count += output;
        if count > pixels {
            return Err(invalid("Alpha RLE exceeds image dimensions"));
        }
    }
    if count != pixels {
        return Err(invalid("Alpha RLE does not fill image dimensions"));
    }
    Ok(())
}
