//! Bounded geometry batches and source-preserving polyhedron attributes.
use super::rendering::field;
use super::*;

// Bounded stored-parameter profile. The SDK reserves 256 floats for two
// width-sized bound vectors; widths above three are explicitly supported.
// These tuples are not assumed to be UV, UVW, or spatial coordinates.
pub(super) const MAX_PARAMETER_WIDTH: u8 = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolyPolylineLayout {
    pub flags: u16,
    pub point_count: u32,
    pub primitive_count: u32,
    pub dimensions: u8,
    pub lengths: SourceSpan,
    pub points: SourceSpan,
    /// SAME/PER_PRIMITIVE axis values, in X/Y/Z order, after unrestricted points.
    pub leftovers: SourceSpan,
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
        // Compression and single-primitive omission are separate wire profiles.
        // Each axis restriction is a two-bit enum, not independent boolean bits.
        if flags & !0x3f02 != 0 {
            return Err(unsupported(
                "hsf.poly_polyline_flags",
                "Only uncompressed batches with stored component lengths are supported",
            ));
        }
        let axes: [u16; 3] = std::array::from_fn(|a| (flags >> (8 + 2 * a)) & 3);
        let dimensions = axes.iter().filter(|mode| **mode == 0).count() as u8;
        if dimensions == 0 {
            return Err(unsupported(
                "hsf.poly_polyline_dimensions",
                "At least one unrestricted axis is required",
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
        let at = c.pos;
        let points = c.array(d, point_count, u32::from(dimensions) * 4)?;
        let leftover_count: u32 = axes
            .iter()
            .map(|mode| match mode {
                2 => 1,
                3 => count,
                _ => 0,
            })
            .sum();
        // At least one axis is unrestricted and each component has >=2 points,
        // so leftover_count <= point_count and the addition cannot overflow u32.
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
        })
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
            let family = match opcode {
                1 | 17 => 0,
                3 | 27 => 1,
                99 => 2,
                _ => {
                    return Err(unsupported(
                        "hsf.shell_attribute",
                        "Unsupported Shell attribute",
                    ));
                }
            };
            if out.iter().any(|a| {
                let previous = match a.opcode {
                    1 | 17 => 0,
                    3 | 27 => 1,
                    _ => 2,
                };
                previous == family
            }) {
                return Err(unsupported(
                    "hsf.shell_attribute",
                    "Duplicate Shell attribute family",
                ));
            }
            // Fixed attributes retain at most six fields. Entity metadata adds
            // per-field charges rather than precharging its maximum on every shell.
            self.budget.charge_output(2048)?;
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
                if opcode == 27 {
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
