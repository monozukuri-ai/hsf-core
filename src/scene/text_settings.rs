//! Stored text and render operands, separate from font selection and rendering.
use super::*;
#[derive(Clone, Debug, Serialize)]
pub struct EnumeratedValue {
    pub index: u8,
    pub name: Option<String>,
    pub interpretation_status: Status,
}
#[derive(Clone, Debug, Serialize)]
pub struct RenderingOperand {
    pub name: String,
    pub encoding: String,
    pub source: SourceSpan,
    pub floats: Vec<f32>,
    pub integers: Vec<i64>,
    pub text: Option<TextValue>,
    pub enumeration: Option<EnumeratedValue>,
    pub decoding_status: Status,
}
#[derive(Clone, Debug, Serialize)]
pub struct FontFlag {
    pub name: String,
    pub bit: u32,
    pub value: bool,
    pub source: SourceSpan,
}
#[derive(Clone, Debug, Serialize)]
pub struct RenderingSettings {
    pub mask: u64,
    pub value: u64,
    pub operands: Vec<RenderingOperand>,
    pub font_flags: Vec<FontFlag>,
    pub decoding_status: Status,
    pub interpretation_status: Status,
}
fn enumeration(index: u8, names: &[&str]) -> EnumeratedValue {
    let name = names.get(usize::from(index)).map(|s| (*s).to_owned());
    EnumeratedValue {
        index,
        interpretation_status: if name.is_some() {
            Status::Complete
        } else {
            Status::Unsupported
        },
        name,
    }
}
impl Builder<'_> {
    pub(super) fn text_alignment(&mut self, r: &Record, n: usize) -> Result<(), HsfError> {
        self.budget.charge_output(4096)?;
        let value = r.value.expect("alignment") as u8;
        let mut complete = true;
        let index = value & 15;
        if index != 15 {
            let e = enumeration(
                index,
                &[
                    "lower_left",
                    "upper_left",
                    "center_left",
                    "lower_right",
                    "upper_right",
                    "center_right",
                    "lower_center",
                    "upper_center",
                    "center",
                    "insertion_point",
                ],
            );
            complete &= e.interpretation_status == Status::Complete;
            self.data.nodes[n].display.text_anchor = Some(Attribute {
                value: e,
                source: r.source.clone(),
            });
        }
        let index = value >> 4;
        if index != 0 {
            let e = enumeration(index, &["no_setting", "left", "center", "right"]);
            complete &= e.interpretation_status == Status::Complete;
            self.data.nodes[n].display.text_justification = Some(Attribute {
                value: e,
                source: r.source.clone(),
            });
        }
        if !complete {
            self.issue(
                r,
                "hsf.rendering_semantics",
                "Unknown text alignment or justification is retained and placements are withheld",
            )?;
        }
        self.data.rendering_options.push(RenderingOptions {
            line_style_binding: None,
            glyph_binding: None,
            settings: None,
            node_id: self.data.nodes[n].id.clone(),
            source_record_id: r.id.clone(),
            source: r.source.clone(),
            kind: r.kind.clone(),
            interpretation_status: if complete {
                Status::Complete
            } else {
                Status::Unsupported
            },
        });
        Ok(())
    }
    pub(super) fn render_settings(
        &mut self,
        r: &Record,
    ) -> Result<Option<RenderingSettings>, HsfError> {
        if !matches!(r.opcode, b'F' | b'H' | b'R') {
            return Ok(None);
        }
        let l = r.rendering.as_ref().expect("render layout");
        let bytes: u64 = l.fields.iter().map(|f| f.source.length).sum();
        self.budget
            .charge_output(8192 + l.fields.len() as u64 * 2048 + bytes * 8)?;
        let mut operands = Vec::new();
        let mut complete = true;
        for f in &l.fields {
            let bytes = self.doc.source_bytes(&f.source).expect("operand span");
            let mut v = RenderingOperand {
                name: f.name.clone(),
                encoding: f.encoding.clone(),
                source: f.source.clone(),
                floats: vec![],
                integers: vec![],
                text: None,
                enumeration: None,
                decoding_status: Status::Complete,
            };
            match f.encoding.as_str() {
                "f32_le" => {
                    v.floats = bytes
                        .chunks_exact(4)
                        .map(|x| f32::from_le_bytes(x.try_into().unwrap()))
                        .collect()
                }
                "u8" => v.integers = bytes.iter().map(|x| i64::from(*x)).collect(),
                "u16_le" => {
                    v.integers = bytes
                        .chunks_exact(2)
                        .map(|x| i64::from(u16::from_le_bytes(x.try_into().unwrap())))
                        .collect()
                }
                "u32_le" => {
                    v.integers = bytes
                        .chunks_exact(4)
                        .map(|x| i64::from(u32::from_le_bytes(x.try_into().unwrap())))
                        .collect()
                }
                "i32_le" => {
                    v.integers = bytes
                        .chunks_exact(4)
                        .map(|x| i64::from(i32::from_le_bytes(x.try_into().unwrap())))
                        .collect()
                }
                "unspecified_text" => {
                    let text = primitive::text(self.doc, &f.source, None, self.budget)?;
                    v.decoding_status = text.status.clone();
                    v.text = Some(text);
                }
                _ => v.decoding_status = Status::Unsupported,
            }
            if r.opcode == b'F' && bytes.len() == 1 {
                let names = match f.name.as_str() {
                    "tolerance_units" => Some(
                        &[
                            "object",
                            "screen",
                            "window",
                            "points",
                            "pixels",
                            "percentage",
                        ][..],
                    ),
                    "size_units" | "extra_space_units" | "greeking_limit_units" => {
                        Some(&["object", "screen", "window", "points", "pixels"][..])
                    }
                    "transforms" => Some(&["position_only", "character_glyphs"][..]),
                    "greeking_mode" => Some(&["none", "lines", "box"][..]),
                    _ => None,
                };
                if let Some(names) = names {
                    let e = enumeration(bytes[0], names);
                    if e.interpretation_status != Status::Complete {
                        v.decoding_status = Status::Unsupported;
                    }
                    v.enumeration = Some(e);
                }
            }
            complete &= v.decoding_status == Status::Complete;
            operands.push(v);
        }
        let mut font_flags = Vec::new();
        if r.opcode == b'F' {
            for (bit, name) in [
                (0x400, "outline"),
                (0x800, "underline"),
                (0x1000, "strikethrough"),
                (0x2000, "overline"),
                (0x4000, "non_proportional"),
                (0x20000, "fill_edges"),
                (0x40000, "bold"),
                (0x80000, "italic"),
            ] {
                if l.mask & u64::from(bit) != 0 {
                    font_flags.push(FontFlag {
                        name: name.into(),
                        bit,
                        value: l.value & u64::from(bit) != 0,
                        source: r.source.clone(),
                    });
                }
            }
        }
        Ok(Some(RenderingSettings {
            mask: l.mask,
            value: l.value,
            operands,
            font_flags,
            decoding_status: if complete {
                Status::Complete
            } else {
                Status::Unsupported
            },
            interpretation_status: Status::Unsupported,
        }))
    }
}
