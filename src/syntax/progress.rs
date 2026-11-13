//! Bounded geometry batches and source-preserving polyhedron attributes.
use super::rendering::field;
use super::*;

// Bounded stored-parameter profile: two width-sized bound vectors of at most
// 128 floats each; widths above three are explicitly supported.
// These tuples are not assumed to be UV, UVW, or spatial coordinates.
pub(super) const MAX_PARAMETER_WIDTH: u8 = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolyPolylineLayout {
    pub flags: u16,
    pub point_count: u32,
    pub primitive_count: u32,
    pub dimensions: u8,
    pub lengths: SourceSpan,
    /// Unrestricted coordinates as f32, or the packed codes when
    /// `point_quantization` is set.
    pub points: SourceSpan,
    /// SAME/PER_PRIMITIVE axis values, in X/Y/Z order, after unrestricted points.
    pub leftovers: SourceSpan,
    /// Trivially quantized points (flag 1): `bounds` holds the minimum, then
    /// the maximum, of each unrestricted axis.
    #[serde(default)]
    pub point_quantization: Option<PointQuantization>,
    /// Quantized SAME/PER_PRIMITIVE values of a compressed batch (one axis):
    /// `bounds` holds their minimum and maximum and `leftovers` the codes,
    /// packed with the points' bit width.
    #[serde(default)]
    pub leftover_quantization: Option<PointQuantization>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellAttributeLayout {
    pub opcode: u8,
    pub source: SourceSpan,
    pub fields: Vec<OperandField>,
}

fn unsupported(code: &str, message: &str) -> HsfError {
    error(code, Status::Unsupported, message)
}
pub(super) fn finite(data: &[u8], code: &str) -> Result<(), HsfError> {
    if data
        .chunks_exact(4)
        .any(|b| !f32::from_le_bytes(b.try_into().unwrap()).is_finite())
    {
        Err(error(
            code,
            Status::Invalid,
            "Geometry values must be finite",
        ))
    } else {
        Ok(())
    }
}
/// The legacy 8-bit xyz encoder writes octets; the general bit-packer writes
/// complete 32-bit words, including its final partial word.
pub(super) fn packed_length(count: u32, width: u8, bits: u8) -> u64 {
    let n = u64::from(count) * u64::from(width);
    if width == 3 && bits == 8 {
        n
    } else {
        (n * u64::from(bits)).div_ceil(32) * 4
    }
}

impl Parser<'_> {
    pub(super) fn poly_polyline(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<PolyPolylineLayout, HsfError> {
        let flags = c.u16()?;
        // Single-primitive omission and global quantization are separate wire
        // profiles. Each axis restriction is a two-bit enum, not independent
        // boolean bits. Compression (1) is framed as trivial quantization.
        if flags & !0x3f03 != 0 {
            return Err(unsupported(
                "hsf.poly_polyline_flags",
                "Only batches with stored component lengths are supported",
            ));
        }
        let axes: [u16; 3] = std::array::from_fn(|a| (flags >> (8 + 2 * a)) & 3);
        let dimensions = axes.iter().filter(|mode| **mode == 0).count() as u8;
        // Without an unrestricted axis every point comes from the shared and
        // per-component values (uncompressed DXF/DWG reader dots at 18.00).
        if dimensions == 0 && flags & 1 != 0 {
            return Err(unsupported(
                "hsf.poly_polyline_dimensions",
                "Quantized batches need an unrestricted axis",
            ));
        }
        let point_count = c.nonnegative()?;
        if point_count < 2 {
            return Err(error(
                "hsf.invalid_poly_polyline",
                Status::Invalid,
                "Batch requires at least two points",
            ));
        }
        charge(
            &mut self.vertices,
            u64::from(point_count),
            self.options.max_vertices,
            "hsf.limit_vertices",
        )?;
        let explicit = if flags & 2 != 0 {
            Some(c.nonnegative()?)
        } else {
            None
        };
        if explicit.is_some_and(|n| n == 0 || n > point_count / 2) {
            return Err(error(
                "hsf.invalid_poly_polyline",
                Status::Invalid,
                "Invalid explicit primitive count",
            ));
        }
        let start = c.pos;
        let mut total = 0u32;
        let mut count = 0u32;
        while explicit.map_or(total < point_count, |n| count < n) {
            let n = c.nonnegative()?;
            if n < 2 || n > point_count - total {
                return Err(error(
                    "hsf.invalid_poly_polyline",
                    Status::Invalid,
                    "Primitive lengths must sum to the point count",
                ));
            }
            total += n;
            count += 1;
            if u64::from(count) > self.options.max_records {
                return Err(error(
                    "hsf.limit_poly_polyline_parts",
                    Status::LimitExceeded,
                    "Batch primitive count exceeds record limit",
                ));
            }
        }
        if total != point_count {
            return Err(error(
                "hsf.invalid_poly_polyline",
                Status::Invalid,
                "Incomplete primitive lengths",
            ));
        }
        let lengths = span(d, start, c.pos - start);
        let leftover_count: u32 = axes
            .iter()
            .map(|mode| match mode {
                2 => 1,
                3 => count,
                _ => 0,
            })
            .sum();
        if flags & 1 != 0 {
            return self.quantized_poly_polyline(
                d,
                c,
                (flags, point_count, count, dimensions),
                lengths,
                leftover_count,
            );
        }
        let at = c.pos;
        let points = c.array(d, point_count, u32::from(dimensions) * 4)?;
        // Each component has >=2 points, so leftover_count <= 3 * point_count / 2
        // and the sum cannot overflow u32.
        let leftovers = c.array(d, leftover_count, 4)?;
        finite(&c.data[at..c.pos], "hsf.nonfinite_poly_polyline")?;
        Ok(PolyPolylineLayout {
            flags,
            point_count,
            primitive_count: count,
            dimensions,
            lengths,
            points,
            leftovers,
            point_quantization: None,
            leftover_quantization: None,
        })
    }

    /// Compressed batch points (flag 1), after the component lengths: the
    /// scheme (only 1, trivial), the box of the unrestricted axes (each
    /// minimum, then each maximum), the bits per coordinate (i32) and the byte
    /// length of the codes, packed as quantized parameters are. The shared or
    /// per-component values of one restricted axis follow as their own block:
    /// the scheme, their minimum and maximum and the byte length of codes
    /// packed with the same bit width. Values of two or more axes and the
    /// octet layout of three 8-bit axes are unverified and stop.
    fn quantized_poly_polyline(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
        (flags, point_count, count, dimensions): (u16, u32, u32, u8),
        lengths: SourceSpan,
        leftover_count: u32,
    ) -> Result<PolyPolylineLayout, HsfError> {
        if c.u8()? != 1 {
            return Err(unsupported(
                "hsf.poly_polyline_compression",
                "Only trivially quantized batch points are supported",
            ));
        }
        let restricted = (0..3).filter(|a| (flags >> (8 + 2 * a)) & 3 >= 2).count();
        if restricted > 1 {
            return Err(unsupported(
                "hsf.poly_polyline_compression",
                "Shared or per-component values of several axes with quantized points are not framed",
            ));
        }
        let at = c.pos;
        let bounds = c.take(d, usize::from(dimensions) * 8)?;
        finite(&c.data[at..c.pos], "hsf.nonfinite_poly_polyline")?;
        let corners: Vec<f32> = c.data[at..c.pos]
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().expect("four bytes")))
            .collect();
        let n = usize::from(dimensions);
        if (0..n).any(|i| corners[i] > corners[i + n]) {
            return Err(error(
                "hsf.invalid_poly_polyline",
                Status::Invalid,
                "Quantized point minimum exceeds maximum",
            ));
        }
        let bits = c.u32()?;
        if !(2..=31).contains(&bits) || (dimensions == 3 && bits == 8) {
            return Err(unsupported(
                "hsf.poly_polyline_compression",
                "Quantized batch points need 2..31 bits, other than three 8-bit axes",
            ));
        }
        let bits = bits as u8;
        let length = c.nonnegative()?;
        if u64::from(length) != packed_length(point_count, dimensions, bits) {
            return Err(error(
                "hsf.invalid_poly_polyline",
                Status::Invalid,
                "Packed point length differs from count, axes and bit width",
            ));
        }
        let points = c.array(d, length, 1)?;
        let (leftovers, leftover_quantization) = if leftover_count == 0 {
            (span(d, c.pos, 0), None)
        } else {
            if c.u8()? != 1 {
                return Err(unsupported(
                    "hsf.poly_polyline_compression",
                    "Only trivially quantized batch values are supported",
                ));
            }
            let at = c.pos;
            let range = c.take(d, 8)?;
            finite(&c.data[at..c.pos], "hsf.nonfinite_poly_polyline")?;
            let low = f32::from_le_bytes(c.data[at..at + 4].try_into().expect("four bytes"));
            let high = f32::from_le_bytes(c.data[at + 4..at + 8].try_into().expect("four bytes"));
            if low > high {
                return Err(error(
                    "hsf.invalid_poly_polyline",
                    Status::Invalid,
                    "Quantized value minimum exceeds maximum",
                ));
            }
            let length = c.nonnegative()?;
            if u64::from(length) != packed_length(leftover_count, 1, bits) {
                return Err(error(
                    "hsf.invalid_poly_polyline",
                    Status::Invalid,
                    "Packed value length differs from count and bit width",
                ));
            }
            let codes = c.array(d, length, 1)?;
            (
                codes,
                Some(PointQuantization {
                    scheme: 1,
                    bits_per_sample: bits,
                    bounds: range,
                }),
            )
        };
        Ok(PolyPolylineLayout {
            flags,
            point_count,
            primitive_count: count,
            dimensions,
            lengths,
            points,
            leftovers,
            point_quantization: Some(PointQuantization {
                scheme: 1,
                bits_per_sample: bits,
                bounds,
            }),
            leftover_quantization,
        })
    }

    /// Compressed colours on every vertex (Shell attributes 5 for faces, 7 for
    /// edges and 9 for markers): the compression scheme (1, trivial), the bits
    /// per channel, the packed length and RGB codes, packed as quantized
    /// parameters are. Only eight bits per channel, the width written by the
    /// DXF/DWG reader's gradient fills, is framed.
    fn vertex_colors(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
        count: u32,
        fields: &mut Vec<OperandField>,
    ) -> Result<(), HsfError> {
        for name in ["compression", "bits_per_sample"] {
            let at = c.pos;
            let value = c.u8()?;
            if (name == "compression" && value != 1) || (name == "bits_per_sample" && value != 8) {
                return Err(unsupported(
                    "hsf.shell_attribute_compression",
                    "Only trivially compressed eight-bit vertex colours are supported",
                ));
            }
            fields.push(OperandField {
                name: name.into(),
                encoding: "u8".into(),
                source: span(d, at, 1),
            });
        }
        let at = c.pos;
        let length = c.nonnegative()?;
        fields.push(OperandField {
            name: "packed_length".into(),
            encoding: "i32_le".into(),
            source: span(d, at, 4),
        });
        if u64::from(length) != packed_length(count, 3, 8) {
            return Err(error(
                "hsf.invalid_shell_attribute_length",
                Status::Invalid,
                "Packed length differs from count, width and bit packing",
            ));
        }
        let source = c.array(d, length, 1)?;
        fields.push(OperandField {
            name: "colors".into(),
            encoding: "packed".into(),
            source,
        });
        Ok(())
    }

    pub(super) fn shell_attributes(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
        count: u32,
    ) -> Result<Vec<ShellAttributeLayout>, HsfError> {
        let mut out: Vec<ShellAttributeLayout> = vec![];
        loop {
            let start = c.pos;
            let opcode = c.u8()?;
            if opcode == 0 {
                break;
            }
            // 28 has the layout of 27 (width, then a float per vertex and
            // component), observed in HSF 20.90 shells with widths 2 and 5. Its
            // meaning is not established, so it is its own family and the scene
            // reports it as an unsupported attribute. 5, 7 and 9 are compressed
            // colours on every vertex, for faces, edges and markers.
            let family = match opcode {
                1 | 17 => 0,
                3 | 27 => 1,
                99 => 2,
                28 => 3,
                5 => 4,
                7 => 5,
                9 => 6,
                _ => {
                    return Err(unsupported(
                        "hsf.shell_attribute",
                        &format!("Unsupported Shell attribute {opcode}"),
                    ));
                }
            };
            if out.iter().any(|a| {
                let previous = match a.opcode {
                    1 | 17 => 0,
                    3 | 27 => 1,
                    28 => 3,
                    5 => 4,
                    7 => 5,
                    9 => 6,
                    _ => 2,
                };
                previous == family
            }) {
                return Err(unsupported(
                    "hsf.shell_attribute",
                    "Duplicate Shell attribute family",
                ));
            }
            // Fixed attributes retain at most six fields (a few hundred bytes with
            // the boxed layout). Entity metadata adds per-field charges rather than
            // precharging its maximum on every shell.
            self.budget.charge_output(256)?;
            let mut fields = vec![];
            if opcode == 99 {
                // Entity collection is nested, and has its own termination.
                loop {
                    let op = c.u8()?;
                    if op == 0 {
                        break;
                    }
                    if ![b'U', 0x16].contains(&op) {
                        return Err(unsupported(
                            "hsf.shell_entity_attribute",
                            "Only entity user/unicode options are framed",
                        ));
                    }
                    if fields.len() >= 32 || fields.len() as u64 >= self.options.max_records {
                        return Err(error(
                            "hsf.limit_shell_entity_attributes",
                            Status::LimitExceeded,
                            "Too many entity metadata fields",
                        ));
                    }
                    self.budget.charge_output(512)?;
                    let length = c.u16()?;
                    let length = if length == u16::MAX {
                        c.u32()?
                    } else {
                        u32::from(length)
                    };
                    let source =
                        self.payload(d, c, u64::from(length) * if op == 0x16 { 2 } else { 1 })?;
                    fields.push(OperandField {
                        name: if op == b'U' {
                            "user_options"
                        } else {
                            "unicode_options"
                        }
                        .into(),
                        encoding: if op == b'U' {
                            "unspecified_text"
                        } else {
                            "utf16_le"
                        }
                        .into(),
                        source,
                    });
                }
            } else if matches!(opcode, 5 | 7 | 9) {
                self.vertex_colors(d, c, count, &mut fields)?;
            } else if opcode == 17 {
                let source = c.array(d, count, 12)?;
                finite(
                    &c.data[source.offset as usize..c.pos],
                    "hsf.nonfinite_shell",
                )?;
                fields.push(OperandField {
                    name: "normals".into(),
                    encoding: "f32_le".into(),
                    source,
                });
            } else {
                let scheme = if matches!(opcode, 1 | 3) {
                    let at = c.pos;
                    let v = c.u8()?;
                    fields.push(OperandField {
                        name: "compression".into(),
                        encoding: "u8".into(),
                        source: span(d, at, 1),
                    });
                    v
                } else {
                    0
                };
                if (opcode == 1 && ![1, 7].contains(&scheme)) || (opcode == 3 && scheme != 1) {
                    return Err(unsupported(
                        "hsf.shell_attribute_compression",
                        "Unknown attribute compression scheme",
                    ));
                }
                let version = (
                    self.document.header.version.major,
                    self.document.header.version.minor,
                );
                let width = if opcode == 1 {
                    if scheme == 7 { 2 } else { 3 }
                } else if opcode == 3 && version < (11, 75) {
                    3
                } else {
                    let at = c.pos;
                    let width = c.u8()?;
                    fields.push(OperandField {
                        name: "width".into(),
                        encoding: "u8".into(),
                        source: span(d, at, 1),
                    });
                    width
                };
                if !(1..=MAX_PARAMETER_WIDTH).contains(&width) {
                    return Err(unsupported(
                        "hsf.shell_parameter_width",
                        "Stored parameter widths must be in the bounded 1..128 profile",
                    ));
                }
                if opcode == 3 {
                    field(
                        d,
                        c,
                        &mut fields,
                        "bounds",
                        "f32_le",
                        usize::from(width) * 8,
                    )?;
                }
                if matches!(opcode, 27 | 28) {
                    let source = c.array(d, count, u32::from(width) * 4)?;
                    finite(
                        &c.data[source.offset as usize..c.pos],
                        "hsf.nonfinite_shell",
                    )?;
                    fields.push(OperandField {
                        name: "parameters".into(),
                        encoding: "f32_le".into(),
                        source,
                    });
                } else {
                    let at = c.pos;
                    let bits = c.u8()?;
                    fields.push(OperandField {
                        name: "bits_per_sample".into(),
                        encoding: "u8".into(),
                        source: span(d, at, 1),
                    });
                    if !(2..=31).contains(&bits) {
                        return Err(unsupported(
                            "hsf.shell_attribute_bits",
                            "Attribute bit width must be 2..31",
                        ));
                    }
                    let at = c.pos;
                    let length = c.nonnegative()?;
                    fields.push(OperandField {
                        name: "packed_length".into(),
                        encoding: "i32_le".into(),
                        source: span(d, at, 4),
                    });
                    if u64::from(length) != packed_length(count, width, bits) {
                        return Err(error(
                            "hsf.invalid_shell_attribute_length",
                            Status::Invalid,
                            "Packed length differs from count, width and bit packing",
                        ));
                    }
                    let source = c.array(d, length, 1)?;
                    fields.push(OperandField {
                        name: if opcode == 1 { "normals" } else { "parameters" }.into(),
                        encoding: "packed".into(),
                        source,
                    });
                }
            }
            out.push(ShellAttributeLayout {
                opcode,
                source: span(d, start, c.pos - start),
                fields,
            });
        }
        Ok(out)
    }
}
