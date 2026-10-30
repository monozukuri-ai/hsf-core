//! Source-preserving framing of render settings. No renderer or string evaluator.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperandField {
    pub name: String,
    pub encoding: String,
    pub source: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderingLayout {
    pub mask: u64,
    /// Heuristics/Text Font/Rendering Options values; Color channel mask.
    pub value: u64,
    /// Rendering Options only: the third mask/value word (TKO_Rendo_Extended2),
    /// zero when absent. Other records never set it.
    #[serde(default)]
    pub extended2_mask: u32,
    #[serde(default)]
    pub extended2_value: u32,
    pub fields: Vec<OperandField>,
}

fn unsupported(code: &str, message: &str) -> HsfError {
    error(code, Status::Unsupported, message)
}
pub(super) fn field(
    d: usize,
    c: &mut Cursor<'_>,
    out: &mut Vec<OperandField>,
    name: &str,
    encoding: &str,
    length: usize,
) -> Result<(), HsfError> {
    let start = c.pos;
    let source = c.take(d, length)?;
    if encoding == "f32_le"
        && c.data[start..c.pos]
            .chunks_exact(4)
            .any(|v| !f32::from_le_bytes(v.try_into().unwrap()).is_finite())
    {
        return Err(error(
            "hsf.nonfinite_rendering",
            Status::Invalid,
            "Rendering operand must be finite",
        ));
    }
    out.push(OperandField {
        name: name.into(),
        encoding: encoding.into(),
        source,
    });
    Ok(())
}
impl Parser<'_> {
    pub(super) fn color_mask(&self, c: &mut Cursor<'_>) -> Result<u32, HsfError> {
        self.color_mask_profile(c, false)
    }
    fn color_mask_profile(&self, c: &mut Cursor<'_>, opaque: bool) -> Result<u32, HsfError> {
        let mut mask = u32::from(c.u8()?);
        for (flag, shift) in [(0x80, 8), (0x8000, 16), (0x800000, 24)] {
            if mask & flag != 0 {
                mask |= u32::from(c.u8()?) << shift;
            }
        }
        let extra = if opaque
            && (
                self.document.header.version.major,
                self.document.header.version.minor,
            ) == (19, 10)
        {
            0x06000000
        } else {
            0
        };
        if mask & !(0x017f7f7f | 0x00808080 | extra) != 0 {
            return Err(unsupported(
                "hsf.color_mask",
                "Unknown color geometry mask bits",
            ));
        }
        Ok(mask)
    }
    fn string_field(
        &self,
        d: usize,
        c: &mut Cursor<'_>,
        out: &mut Vec<OperandField>,
        name: &str,
        length: u64,
    ) -> Result<(), HsfError> {
        let source = self.payload(d, c, length)?;
        out.push(OperandField {
            name: name.into(),
            encoding: "unspecified_text".into(),
            source,
        });
        Ok(())
    }
    pub(super) fn rendering(
        &mut self,
        op: u8,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<RenderingLayout, HsfError> {
        // At most 32 fixed/name fields; charge before retaining names/spans.
        self.budget.charge_output(16384)?;
        let v = &self.document.header.version;
        let version = (v.major, v.minor);
        let mut fields = Vec::new();
        let (mask, value) = match op {
            b'H' => {
                let mut mask = u32::from(c.u16()?);
                if mask & 0x8000 != 0 {
                    mask |= u32::from(c.u16()?) << 16;
                }
                let mut value = u32::from(c.u16()?);
                if mask & 0x8000 != 0 {
                    value |= u32::from(c.u16()?) << 16;
                }
                if mask & !0x03ffffff != 0 {
                    return Err(unsupported(
                        "hsf.heuristics_mask",
                        "Unknown heuristics mask",
                    ));
                }
                if (version < (12, 1) && mask & 0x10000 != 0)
                    || (version < (14, 1) && mask & 0x600000 != 0)
                    || (version < (15, 15) && mask & 0x1000000 != 0)
                {
                    return Err(unsupported(
                        "hsf.heuristics_version",
                        "Heuristics fields are unavailable in this version",
                    ));
                }
                let enabled = mask & value;
                for (bit, name) in [(0x40, "related_limit"), (0x80, "internal_shell_limit")] {
                    if enabled & bit != 0 {
                        field(d, c, &mut fields, name, "i32_le", 4)?;
                    }
                }
                if enabled & 12 != 0 {
                    field(d, c, &mut fields, "details", "u8", 1)?;
                }
                let mut culling = 0u32;
                if enabled & 0x10000 != 0 {
                    let start = c.pos;
                    culling = u32::from(c.u8()?);
                    if culling & 128 != 0 {
                        culling |= u32::from(c.u8()?) << 8;
                    }
                    if culling & !0x3ff7 != 0 {
                        return Err(unsupported(
                            "hsf.heuristics_culling",
                            "Unknown culling bits",
                        ));
                    }
                    if (version < (14, 15) && culling & 128 != 0)
                        || (version < (15, 10) && culling & 0x200 != 0)
                        || (version < (15, 50) && culling & 0xc00 != 0)
                    {
                        return Err(unsupported(
                            "hsf.heuristics_version",
                            "Culling fields are unavailable in this version",
                        ));
                    }
                    fields.push(OperandField {
                        name: "culling".into(),
                        encoding: "uint_le".into(),
                        source: span(d, start, c.pos - start),
                    });
                    for (bit, name) in [
                        (2, "obscuration_threshold"),
                        (4, "maximum_extent"),
                        (0x200, "maximum_extent_mode"),
                    ] {
                        if culling & bit != 0 {
                            field(d, c, &mut fields, name, "i32_le", 4)?;
                        }
                    }
                }
                if mask & 0x200000 != 0 {
                    let at = c.pos;
                    let weights = c.u8()?;
                    if weights & !63 != 0 {
                        return Err(unsupported(
                            "hsf.heuristics_weights",
                            "Unknown ordered weight bits",
                        ));
                    }
                    fields.push(OperandField {
                        name: "ordered_weights_mask".into(),
                        encoding: "u8".into(),
                        source: span(d, at, 1),
                    });
                    for bit in 0..6 {
                        if weights & (1 << bit) != 0 {
                            field(
                                d,
                                c,
                                &mut fields,
                                &format!("ordered_weight_{bit}"),
                                "f32_le",
                                4,
                            )?;
                        }
                    }
                }
                if enabled & 0x400000 != 0 {
                    field(d, c, &mut fields, "internal_polyline_limit", "i32_le", 4)?;
                }
                if mask & 0x1000000 != 0 {
                    field(d, c, &mut fields, "selection_level", "u8", 1)?;
                }
                if culling & 0x400 != 0 {
                    field(d, c, &mut fields, "culling_vector", "f32_le", 12)?;
                }
                if culling & 0x800 != 0 {
                    field(d, c, &mut fields, "culling_tolerance", "f32_le", 4)?;
                }
                (mask, value)
            }
            b'F' => {
                let mut mask = u32::from(c.u8()?);
                let mut value = u32::from(c.u8()?);
                if mask & 128 != 0 {
                    mask |= u32::from(c.u8()?) << 8;
                    value |= u32::from(c.u8()?) << 8;
                }
                if mask & 0x8000 != 0 {
                    mask |= u32::from(c.u16()?) << 16;
                    value |= u32::from(c.u16()?) << 16;
                }
                if mask & !0x7fffff != 0 {
                    return Err(unsupported("hsf.text_font_mask", "Unknown text font mask"));
                }
                if (version < (12, 10) && mask & !0x7fff != 0)
                    || (version < (12, 20) && mask & !0x3ffff != 0)
                    || (version < (13, 25) && mask & !0x1fffff != 0)
                {
                    return Err(unsupported(
                        "hsf.text_font_version",
                        "Text font fields are unavailable in this version",
                    ));
                }
                let enabled = mask & value;
                if enabled & 1 != 0 {
                    let n = u64::from(c.u8()?);
                    self.string_field(d, c, &mut fields, "names", n)?;
                }
                for (bit, name, encoding, length, units) in [
                    (2, "size", "f32_le", 4, true),
                    (4, "tolerance", "f32_le", 4, true),
                    (8, "transforms", "u8", 1, false),
                    (16, "rotation", "f32_le", 4, false),
                    (32, "slant", "f32_le", 4, false),
                    (64, "width_scale", "f32_le", 4, false),
                    (256, "extra_space", "f32_le", 4, true),
                    (512, "line_spacing", "f32_le", 4, false),
                    (0x10000, "greeking_limit", "f32_le", 4, true),
                    (0x200000, "greeking_mode", "u8", 1, false),
                ] {
                    if enabled & bit != 0 {
                        field(d, c, &mut fields, name, encoding, length)?;
                        if units {
                            field(d, c, &mut fields, &format!("{name}_units"), "u8", 1)?;
                        }
                    }
                }
                for (bit, name) in [(0x100000, "renderer"), (0x400000, "preference")] {
                    if mask & bit != 0 {
                        field(d, c, &mut fields, name, "u8", 1)?;
                        if version >= (13, 25) {
                            field(d, c, &mut fields, &format!("{name}_cutoff"), "f32_le", 4)?;
                            field(d, c, &mut fields, &format!("{name}_cutoff_units"), "u8", 1)?;
                        }
                    }
                }
                (mask, value)
            }
            b'"' => {
                let mask = self.color_mask_profile(c, true)?;
                let mut channels = u32::from(c.u8()?);
                if channels & 128 != 0 {
                    channels |= u32::from(c.u8()?) << 8;
                }
                if channels & !0x3ff != 0 {
                    return Err(unsupported(
                        "hsf.color_channels",
                        "Unknown material channels",
                    ));
                }
                for (bit, name) in [
                    (1, "diffuse"),
                    (2, "specular"),
                    (4, "mirror"),
                    (8, "transmission"),
                    (16, "emission"),
                    (32, "gloss"),
                    (64, "index"),
                    (256, "environment"),
                    (512, "bump"),
                ] {
                    if channels & bit == 0 {
                        continue;
                    }
                    if bit == 32 || bit == 64 {
                        field(d, c, &mut fields, name, "f32_le", 4)?;
                        continue;
                    }
                    let mut n = u64::from(c.u8()?);
                    if bit == 1 && n == 255 {
                        n = u64::from(c.nonnegative()?);
                    }
                    if n == 0 && bit < 32 {
                        field(d, c, &mut fields, name, "rgb8", 3)?;
                    } else {
                        self.string_field(d, c, &mut fields, name, n)?;
                    }
                }
                (mask, channels)
            }
            _ => unreachable!(),
        };
        Ok(RenderingLayout {
            mask: u64::from(mask),
            value: u64::from(value),
            extended2_mask: 0,
            extended2_value: 0,
            fields,
        })
    }
}
