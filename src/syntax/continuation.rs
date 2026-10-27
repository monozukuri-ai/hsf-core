//! Framing for scene continuation, texture definitions, and rectangular meshes.
use super::rendering::field;
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraLayout {
    pub projection: u8,
    pub settings: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureLayout {
    pub name: SourceSpan,
    pub image: SourceSpan,
    pub flags: u32,
    pub fields: Vec<OperandField>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedColorLayout {
    pub geometry_mask: u32,
    pub index: u16,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridMeshLayout {
    pub flags: u8,
    pub rows: u32,
    pub columns: u32,
    pub vertex_count: u32,
    pub vertices: SourceSpan,
    pub attributes: Vec<GridAttributeLayout>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridAttributeLayout {
    pub opcode: u8,
    pub source: SourceSpan,
    pub width: u8,
    pub bits_per_sample: Option<u8>,
    pub bounds: Option<SourceSpan>,
    /// Raw f32s or packed octets; texture coordinates are not evaluated.
    pub values: SourceSpan,
}

fn unsupported(code: &str, message: &str) -> HsfError {
    error(code, Status::Unsupported, message)
}

fn finite(data: &[u8]) -> Result<(), HsfError> {
    if data
        .chunks_exact(4)
        .any(|b| !f32::from_le_bytes(b.try_into().unwrap()).is_finite())
    {
        Err(error(
            "hsf.nonfinite_mesh",
            Status::Invalid,
            "Mesh values must be finite",
        ))
    } else {
        Ok(())
    }
}

impl Parser<'_> {
    pub(super) fn texture(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<TextureLayout, HsfError> {
        self.budget.charge_output(16384)?;
        let mut text = || -> Result<SourceSpan, HsfError> {
            let length = match c.u8()? {
                255 => c.nonnegative()?,
                n => u32::from(n),
            };
            self.payload(d, c, u64::from(length))
        };
        let name = text()?;
        let image = text()?;
        let mut flags = u32::from(c.u16()?);
        if flags & 0x8000 != 0 {
            let v = &self.document.header.version;
            if (v.major, v.minor) < (11, 65) {
                return Err(unsupported(
                    "hsf.texture_version",
                    "Extended texture options require HSF 11.65",
                ));
            }
            flags |= u32::from(c.u16()?) << 16;
        }
        if flags & !0x7ffff != 0 {
            return Err(unsupported("hsf.texture_flags", "Unknown texture flags"));
        }
        let mut fields = vec![];
        for (bit, name) in [
            (1, "parameter_source"),
            (2, "tiling"),
            (4, "interpolation"),
            (8, "decimation"),
            (16, "red_mapping"),
            (32, "green_mapping"),
            (64, "blue_mapping"),
            (128, "alpha_mapping"),
            (256, "parameter_function"),
            (512, "layout"),
        ] {
            if flags & bit != 0 {
                field(d, c, &mut fields, name, "u8", 1)?;
            }
        }
        if flags & 0x800 != 0 {
            field(d, c, &mut fields, "value_scale", "f32_le", 8)?;
        }
        if flags & 0x400 != 0 {
            let length = u64::from(c.u8()?);
            fields.push(OperandField {
                name: "transform_segment".into(),
                encoding: "unspecified_text".into(),
                source: self.payload(d, c, length)?,
            });
        }
        if flags & 0x30000 != 0 {
            field(d, c, &mut fields, "apply_mode", "u8", 1)?;
        }
        if flags & 0x40000 != 0 {
            field(d, c, &mut fields, "parameter_offset", "u8", 1)?;
        }
        Ok(TextureLayout {
            name,
            image,
            flags,
            fields,
        })
    }

    pub(super) fn grid_mesh(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<GridMeshLayout, HsfError> {
        let flags = c.u8()?;
        if flags & !8 != 0 {
            return Err(unsupported(
                "hsf.mesh_flags",
                "Only uncompressed Mesh points with bounded optional attributes are supported",
            ));
        }
        let rows = c.nonnegative()?;
        let columns = c.nonnegative()?;
        if rows < 2 || columns < 2 {
            return Err(unsupported(
                "hsf.mesh_dimensions",
                "Mesh profile requires at least two rows and columns",
            ));
        }
        let vertex_count = rows
            .checked_mul(columns)
            .filter(|n| *n <= i32::MAX as u32)
            .ok_or_else(|| {
                error(
                    "hsf.invalid_mesh_count",
                    Status::Invalid,
                    "Mesh dimensions overflow the point count",
                )
            })?;
        charge(
            &mut self.vertices,
            u64::from(vertex_count),
            self.options.max_vertices,
            "hsf.limit_vertices",
        )?;
        let cells = u64::from(rows - 1) * u64::from(columns - 1);
        // Two generated triangle faces per cell, each with 3 IDs plus a count.
        charge(
            &mut self.face_values,
            cells * 8,
            self.options.max_face_values,
            "hsf.limit_face_values",
        )?;
        let at = c.pos;
        // Unlike Shell, Mesh has no separate point-count word here.
        let vertices = c.array(d, vertex_count, 12)?;
        finite(&c.data[at..c.pos])?;
        let mut attributes: Vec<GridAttributeLayout> = vec![];
        if flags & 8 != 0 {
            loop {
                let start = c.pos;
                let opcode = c.u8()?;
                if opcode == 0 {
                    break;
                }
                if ![3, 17, 27].contains(&opcode)
                    || attributes.iter().any(|a| {
                        a.opcode == opcode
                            || (a.opcode == 3 && opcode == 27)
                            || (a.opcode == 27 && opcode == 3)
                    })
                {
                    return Err(unsupported(
                        "hsf.mesh_attribute",
                        "Only one all-vertex normal and parameter array is supported",
                    ));
                }
                self.budget.charge_output(2048)?;
                let version = (
                    self.document.header.version.major,
                    self.document.header.version.minor,
                );
                if opcode == 3 && c.u8()? != 1 {
                    return Err(unsupported(
                        "hsf.mesh_parameter_compression",
                        "Only CS_TRIVIAL parameter framing is supported",
                    ));
                }
                let width = if opcode == 17 || (opcode == 3 && version < (11, 75)) {
                    3
                } else {
                    c.u8()?
                };
                if !(1..=super::progress::MAX_PARAMETER_WIDTH).contains(&width) {
                    return Err(unsupported(
                        "hsf.mesh_parameter_width",
                        "Stored parameter widths must be in the bounded 1..128 profile",
                    ));
                }
                let mut bounds = None;
                let mut bits_per_sample = None;
                if opcode == 3 {
                    let at = c.pos;
                    bounds = Some(c.take(d, usize::from(width) * 8)?);
                    finite(&c.data[at..c.pos])?;
                    let bits = c.u8()?;
                    if bits != 8 {
                        return Err(unsupported(
                            "hsf.mesh_parameter_bits",
                            "Only octet-quantized parameters are framed",
                        ));
                    }
                    bits_per_sample = Some(bits);
                    let length = c.nonnegative()?;
                    if u64::from(length)
                        != super::progress::packed_length(vertex_count, width, bits)
                    {
                        return Err(error(
                            "hsf.invalid_mesh_parameters",
                            Status::Invalid,
                            "Packed parameter length differs from point count and width",
                        ));
                    }
                }
                let at = c.pos;
                let values = if opcode == 3 {
                    c.take(
                        d,
                        super::progress::packed_length(vertex_count, width, 8) as usize,
                    )?
                } else {
                    c.array(d, vertex_count, u32::from(width) * 4)?
                };
                if opcode != 3 {
                    finite(&c.data[at..c.pos])?;
                }
                attributes.push(GridAttributeLayout {
                    opcode,
                    source: span(d, start, c.pos - start),
                    width,
                    bits_per_sample,
                    bounds,
                    values,
                });
            }
        }
        Ok(GridMeshLayout {
            flags,
            rows,
            columns,
            vertex_count,
            vertices,
            attributes,
        })
    }
}
