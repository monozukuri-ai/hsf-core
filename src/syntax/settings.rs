//! Bounded attribute framing. Names, clipping and renderer effects stay opaque.
use super::rendering::field;
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedStyleLayout {
    pub index: Option<u16>,
    pub name: Option<SourceSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipRegionLayout {
    pub options: u8,
    pub point_count: u32,
    pub points: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserIndexLayout {
    pub count: u32,
    /// Signed 32-bit arrays, stored separately (not interleaved pairs).
    pub indices: SourceSpan,
    pub values: SourceSpan,
}

/// One TKE_User_Index_Data entry: a signed application index and opaque bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserIndexDataEntry {
    pub index: i32,
    pub data: SourceSpan,
}

/// TKE_User_Index_Data (`m`, written by HSF 17.80+ toolkits): count, then
/// interleaved (index i32, size i32, data[size]) entries, unlike User Index.
/// Text encodings remain unspecified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserIndexDataLayout {
    pub count: u32,
    pub entries: Vec<UserIndexDataEntry>,
}

fn limit(value: u64, maximum: u64, code: &str) -> Result<(), HsfError> {
    if value > maximum {
        Err(error(
            code,
            Status::LimitExceeded,
            "Attribute count exceeds limit",
        ))
    } else {
        Ok(())
    }
}

fn unsupported(code: &str, message: &str) -> HsfError {
    error(code, Status::Unsupported, message)
}

/// Retain a mask as a field while using its value for bounded framing.
fn uint_field(
    d: usize,
    c: &mut Cursor<'_>,
    out: &mut Vec<OperandField>,
    name: &str,
    width: usize,
) -> Result<u32, HsfError> {
    let at = c.pos;
    field(d, c, out, name, "uint_le", width)?;
    Ok(c.data[at..c.pos]
        .iter()
        .enumerate()
        .fold(0, |v, (i, b)| v | (u32::from(*b) << (8 * i))))
}

impl Parser<'_> {
    pub(super) fn named_style(
        &self,
        op: u8,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<NamedStyleLayout, HsfError> {
        let low = c.u8()?;
        let v = &self.document.header.version;
        if low == 255 && (v.major, v.minor) >= (11, 60) {
            let length = u64::from(c.u8()?);
            return Ok(NamedStyleLayout {
                index: None,
                name: Some(self.payload(d, c, length)?),
            });
        }
        if low == 255 && op == b'@' {
            return Err(unsupported(
                "hsf.marker_symbol_version",
                "Named marker symbols require HSF 11.60 or later",
            ));
        }
        let index = u16::from(low)
            | if op == b'@' {
                0
            } else {
                u16::from(c.u8()?) << 8
            };
        Ok(NamedStyleLayout {
            index: Some(index),
            name: None,
        })
    }

    pub(super) fn clip_region(
        &self,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<ClipRegionLayout, HsfError> {
        let options = c.u8()?;
        // The 2026 DXF writer emits 0x05 with the same fixed count/XYZ framing.
        // Its extra option remains opaque; clip evaluation is not implemented.
        let observed_opaque = options == 5
            && (
                self.document.header.version.major,
                self.document.header.version.minor,
            ) == (19, 10);
        if options & !1 != 0 && !observed_opaque {
            return Err(unsupported(
                "hsf.clip_region_options",
                "Unknown clip region options",
            ));
        }
        let point_count = c.nonnegative()?;
        limit(
            u64::from(point_count),
            self.options.max_vertices,
            "hsf.limit_vertices",
        )?;
        let start = c.pos;
        let points = c.array(d, point_count, 12)?;
        if c.data[start..c.pos]
            .chunks_exact(4)
            .any(|v| !f32::from_le_bytes(v.try_into().unwrap()).is_finite())
        {
            return Err(error(
                "hsf.nonfinite_clip_region",
                Status::Invalid,
                "Clip region coordinates must be finite",
            ));
        }
        Ok(ClipRegionLayout {
            options,
            point_count,
            points,
        })
    }

    pub(super) fn user_index(
        &self,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<UserIndexLayout, HsfError> {
        let count = c.nonnegative()?;
        limit(
            u64::from(count),
            self.options.max_records,
            "hsf.limit_user_indices",
        )?;
        let indices = c.array(d, count, 4)?;
        let values = c.array(d, count, 4)?;
        Ok(UserIndexLayout {
            count,
            indices,
            values,
        })
    }

    pub(super) fn user_index_data(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<UserIndexDataLayout, HsfError> {
        // The 3DF 20.10 reader reads a count, then per entry an index, a size and
        // that many bytes; it rejects counts above 0x1000000. Sizes are read one
        // at a time, so a cut entry is invalid rather than a different layout.
        let count = c.nonnegative()?;
        limit(
            u64::from(count),
            self.options.max_records,
            "hsf.limit_user_index_data",
        )?;
        let mut entries = Vec::new();
        for _ in 0..count {
            self.budget.charge_output(64)?;
            let index = c.u32()? as i32;
            let size = c.nonnegative()?;
            let data = c.array(d, size, 1)?;
            entries.push(UserIndexDataEntry { index, data });
        }
        Ok(UserIndexDataLayout { count, entries })
    }

    pub(super) fn render_options(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<RenderingLayout, HsfError> {
        // At most 96 fixed fields including hidden-line and simple-shadow options.
        self.budget.charge_output(49152)?;
        let v = &self.document.header.version;
        let version = (v.major, v.minor);
        let mask = c.u32()?;
        let high_mask = if mask & 0x80000000 != 0 { c.u32()? } else { 0 };
        // 3DF 20.10 writers (eDrawings 2026) chain a third word behind the second
        // when its bit 31 (TKO_Rendo_Extended2) is set. The three masks precede the
        // three values; 19.10 headers carry it without any version gate.
        let more_mask = if high_mask & 0x80000000 != 0 {
            c.u32()?
        } else {
            0
        };
        let value = c.u32()?;
        let high_value = if mask & 0x80000000 != 0 { c.u32()? } else { 0 };
        let more_value = if high_mask & 0x80000000 != 0 {
            c.u32()?
        } else {
            0
        };
        // Complex nested profiles are not skippable: stop at the original R.
        const HIGH: u32 = 4
            | 0x100
            | 0x10
            | 0x20
            | 0x40
            | 0x80
            | 0x400
            | 0x800
            | 0x1000
            | 0x4000
            | 0x8000
            | 0x10000
            | 0x20000
            // Interpolation and depth-writing switches carry no operand (the SDK
            // class has no member for them): index markers, Gooch lighting,
            // transparency depth writing.
            | 0x1000000
            | 0x4000000
            | 0x8000000
            | 0x10000000
            | 0x80000000;
        // Third word: only switches without operands are known from the SDK header
        // (frame buffer effects, scaled displacement, bump mapping parallax,
        // randomize vertices). Anything with operands (forced lock, contour,
        // isoline, tints, edge join cutoff) has an unverified position: stop.
        const MORE: u32 = 2 | 4 | 0x100 | 0x200;
        if mask & 0x1c000000 != 0 || high_mask & !HIGH != 0 || more_mask & !MORE != 0 {
            return Err(unsupported(
                "hsf.rendering_options_mask",
                "Rendering Options contains an unsupported nested profile or mask",
            ));
        }
        for (bits, minimum) in [
            (0x100, (10, 2)),
            (0x60, (10, 2)),
            (0x400, (12, 10)),
            (0x4000, (13, 35)),
            (0x8000, (14, 5)),
            (0x10000, (15, 5)),
            (0x20000, (15, 10)),
        ] {
            if high_mask & bits != 0 && version < minimum {
                return Err(unsupported(
                    "hsf.rendering_options_version",
                    "Rendering Options field is unavailable in this version",
                ));
            }
        }
        let active = mask & value;
        let high_active = high_mask & high_value;
        let mut fields = Vec::new();
        for (bits, name) in [(0x3000, "hsr"), (0xc0000, "technology_quantization")] {
            // SDK binary grammar uses mask here, even when the value is off.
            if mask & bits != 0 {
                field(d, c, &mut fields, name, "u8", 1)?;
            }
        }
        if active & 0x200000 != 0 {
            field(d, c, &mut fields, "face_displacement", "i32_le", 4)?;
        }
        if active & 0x100000 != 0 {
            let locks = uint_field(d, c, &mut fields, "lock_mask", 4)?;
            let locked = uint_field(d, c, &mut fields, "lock_value", 4)?;
            if locks & !0x7fffffff != 0 {
                return Err(unsupported(
                    "hsf.rendering_lock_mask",
                    "Unknown attribute lock mask",
                ));
            }
            if locks & locked & 4 != 0 {
                let cm = uint_field(d, c, &mut fields, "color_lock_mask", 4)?;
                let cv = uint_field(d, c, &mut fields, "color_lock_value", 4)?;
                // 19.10 files also set selector bits 25/26. The fixed SDK reader
                // consumes no extra channel operands for them; retain them raw.
                let known = if version == (19, 10) {
                    0x077f7f7f
                } else {
                    0x017f7f7f
                };
                if (cm | cv) & !known != 0 {
                    return Err(unsupported(
                        "hsf.rendering_color_lock_mask",
                        "Unknown color lock selector",
                    ));
                }
                if version > (11, 5) {
                    let text_contrast = 0x1000000 | if version < (15, 55) { 0x100000 } else { 0 };
                    for (bits, name, minimum) in [
                        (0x1001, "face", (0, 0)),
                        (0x2000, "back", (0, 0)),
                        (2, "edge", (0, 0)),
                        (4, "line", (0, 0)),
                        (16, "text", (0, 0)),
                        (8, "marker", (0, 0)),
                        (0x4000, "vertex", (0, 0)),
                        (32, "window", (0, 0)),
                        (0x400, "face_contrast", (0, 0)),
                        (0x10000, "edge_contrast", (0, 0)),
                        (0x20000, "line_contrast", (0, 0)),
                        (text_contrast, "text_contrast", (0, 0)),
                        (0x40000, "marker_contrast", (0, 0)),
                        (0x80000, "vertex_contrast", (0, 0)),
                        (0x800, "window_contrast", (0, 0)),
                        (0x200000, "simple_reflection", (15, 50)),
                        (0x400000, "cut_face", (12, 20)),
                        (0x100000, "cut_edge", (12, 20)),
                    ] {
                        // Channel fields depend on color VALUE, not mask & value.
                        if cv & bits != 0 && version >= minimum {
                            uint_field(d, c, &mut fields, &format!("{name}_lock_mask"), 2)?;
                            uint_field(d, c, &mut fields, &format!("{name}_lock_value"), 2)?;
                        }
                    }
                }
            }
            if locks & locked & 0x8000000 != 0 {
                uint_field(d, c, &mut fields, "visibility_lock_mask", 4)?;
                uint_field(d, c, &mut fields, "visibility_lock_value", 4)?;
            }
        }
        for (bits, name, encoding, n) in [
            (0x400000, "fog_limits", "f32_le", 8),
            (0x20000, "debug", "i32_le", 4),
            (0x40000000, "stereo_separation", "f32_le", 4),
        ] {
            if active & bits != 0 {
                field(d, c, &mut fields, name, encoding, n)?;
            }
        }
        if high_active & 0x20000 != 0 {
            field(d, c, &mut fields, "stereo_distance", "f32_le", 4)?;
        }
        if active & 0x800000 != 0 {
            let bm = uint_field(d, c, &mut fields, "buffer_options_mask", 1)?;
            let bv = uint_field(d, c, &mut fields, "buffer_options_value", 1)?;
            if bm & !15 != 0 {
                return Err(unsupported(
                    "hsf.rendering_buffer_mask",
                    "Unknown buffer options mask",
                ));
            }
            if bm & bv & 1 != 0 {
                field(d, c, &mut fields, "buffer_size_limit", "i32_le", 4)?;
            }
        }
        if active & 0x1000000 != 0 {
            self.hidden_line(d, c, &mut fields)?;
        }
        for (bits, name, encoding, n) in [
            (0x10, "depth_range", "f32_le", 8),
            (0x20, "mask_transform", "u8", 1),
            (0x40, "image_scale", "f32_le", 8),
        ] {
            if high_active & bits != 0 {
                field(d, c, &mut fields, name, encoding, n)?;
            }
        }
        if high_active & 0x100 != 0 {
            self.simple_shadow(d, c, &mut fields)?;
        }
        for (bits, name, encoding, n) in [
            (0x400, "image_tint", "rgb8", 3),
            (0x4000, "general_displacement", "i32_le", 4),
            (0x8000, "join_cutoff_angle", "i32_le", 4),
            (0x10000, "screen_range", "f32_le", 16),
        ] {
            if high_active & bits != 0 {
                field(d, c, &mut fields, name, encoding, n)?;
            }
        }
        if active & 0x10000 != 0 && version >= (15, 15) {
            field(d, c, &mut fields, "display_list_level", "u8", 1)?;
        }
        Ok(RenderingLayout {
            mask: u64::from(mask) | (u64::from(high_mask) << 32),
            value: u64::from(value) | (u64::from(high_value) << 32),
            extended2_mask: more_mask,
            extended2_value: more_value,
            fields,
        })
    }

    fn hidden_line(
        &self,
        d: usize,
        c: &mut Cursor<'_>,
        fields: &mut Vec<OperandField>,
    ) -> Result<(), HsfError> {
        let at = c.pos;
        let mut flags = u32::from(c.u8()?);
        if flags & 128 != 0 {
            flags |= u32::from(c.u8()?) << 8;
        }
        if flags & 0x8000 != 0 {
            if (
                self.document.header.version.major,
                self.document.header.version.minor,
            ) < (11, 50)
            {
                return Err(unsupported(
                    "hsf.hidden_line_version",
                    "Extended hidden-line fields require HSF 11.50",
                ));
            }
            flags |= u32::from(c.u16()?) << 16;
        }
        if flags & !0x1f83ff != 0 {
            return Err(unsupported(
                "hsf.hidden_line_mask",
                "Unknown hidden-line options",
            ));
        }
        fields.push(OperandField {
            name: "hidden_line_options".into(),
            encoding: "uint_le".into(),
            source: span(d, at, c.pos - at),
        });
        for (bit, name, encoding, n) in [
            (4, "hidden_line_pattern", "i32_le", 4),
            (16, "hidden_line_dim_factor", "f32_le", 4),
            (8, "hidden_line_face_displacement", "f32_le", 4),
            (0x10000, "hidden_line_color", "f32_le", 12),
        ] {
            if flags & bit != 0 {
                field(d, c, fields, name, encoding, n)?;
            }
        }
        if flags & 0x20000 != 0 {
            let at = c.pos;
            field(d, c, fields, "hidden_line_weight", "f32_le", 4)?;
            let weight = f32::from_le_bytes(c.data[at..at + 4].try_into().unwrap());
            // This nested grammar uses -1 as its sentinel, unlike Edge Weight.
            if weight != -1. {
                field(d, c, fields, "hidden_line_weight_units", "u8", 1)?;
            }
        }
        if flags & 0x100000 != 0 {
            field(d, c, fields, "hidden_line_hsr", "u8", 1)?;
        }
        Ok(())
    }

    fn simple_shadow(
        &self,
        d: usize,
        c: &mut Cursor<'_>,
        fields: &mut Vec<OperandField>,
    ) -> Result<(), HsfError> {
        let at = c.pos;
        let mut flags = u32::from(c.u8()?);
        let version = (
            self.document.header.version.major,
            self.document.header.version.minor,
        );
        if flags & 128 != 0 {
            if version < (11, 70) {
                return Err(unsupported(
                    "hsf.simple_shadow_version",
                    "Extended shadows require HSF 11.70",
                ));
            }
            flags |= u32::from(c.u8()?) << 8;
        }
        if flags & !0x3ff != 0 {
            return Err(unsupported(
                "hsf.simple_shadow_mask",
                "Unknown simple-shadow options",
            ));
        }
        if flags & 0x200 != 0 && version < (14, 10) {
            return Err(unsupported(
                "hsf.simple_shadow_version",
                "Shadow opacity requires HSF 14.10",
            ));
        }
        fields.push(OperandField {
            name: "simple_shadow_options".into(),
            encoding: "uint_le".into(),
            source: span(d, at, c.pos - at),
        });
        for (bit, name, encoding, n) in [
            (4, "shadow_plane", "f32_le", 16),
            (8, "shadow_light", "f32_le", 12),
            (16, "shadow_color", "rgb8", 3),
            (64, "shadow_blur", "u8", 1),
            (32, "shadow_resolution", "uint_le", 2),
            (0x200, "shadow_opacity", "f32_le", 4),
        ] {
            if flags & bit != 0 {
                field(d, c, fields, name, encoding, n)?;
            }
        }
        Ok(())
    }
}
