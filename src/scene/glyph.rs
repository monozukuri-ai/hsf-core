//! Framed glyph instructions and a deliberately bounded marker binding profile.
use super::*;
#[derive(Clone, Debug, Serialize)]
pub struct GlyphDefinition {
    pub id: String,
    pub source_record_id: String,
    pub context_node_id: String,
    pub source: SourceSpan,
    pub name: TextValue,
    pub definition_source: SourceSpan,
    pub program: Option<GlyphProgram>,
    pub interpretation_status: Status,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct GlyphProgram {
    pub nominal_radius: u8,
    pub offset: [i16; 2],
    pub header_source: SourceSpan,
    pub commands: Vec<GlyphCommand>,
    pub termination_source: SourceSpan,
}
#[derive(Clone, Debug, Serialize)]
pub struct GlyphCommand {
    pub code: i16,
    pub kind: String,
    pub points: Vec<[i16; 2]>,
    pub color: Vec<u8>,
    pub source: SourceSpan,
    pub payload_source: SourceSpan,
}
pub type GlyphBinding = LineStyleBinding;
pub(super) struct PendingGlyph {
    pub node: usize,
    pub event: usize,
    pub name: SourceSpan,
}
fn name_allowed(name: &[u8]) -> bool {
    line_style::custom_name(name)
        && !patterns::SYMBOLS
            .iter()
            .any(|s| name.eq_ignore_ascii_case(s.as_bytes()))
}
fn local_span(s: &SourceSpan, start: usize, count: usize) -> SourceSpan {
    SourceSpan {
        domain_id: s.domain_id.clone(),
        offset: s.offset + start as u64,
        length: count as u64,
    }
}
impl Builder<'_> {
    pub(super) fn glyph_definition(&mut self, r: &Record, n: usize) -> Result<(), HsfError> {
        limit(
            self.definition_count() as u64 + 1,
            self.options.max_definitions,
            "hsf.limit_definitions",
        )?;
        self.budget.charge_output(4096)?;
        let l = r.glyph.as_ref().expect("glyph");
        let name = primitive::text(self.doc, &l.name, None, self.budget)?;
        let (program, program_reason) = decode(self.doc, &l.definition, self.budget)?;
        let reason = if !name_allowed(self.doc.source_bytes(&l.name).expect("name")) {
            Some("hsf.glyph_name")
        } else {
            program_reason
        };
        if reason.is_some() {
            self.issue(
                r,
                "hsf.glyph_semantics",
                "Glyph bytes are retained; unsupported drawing state is not applied",
            )?;
        }
        self.data.glyphs.push(GlyphDefinition {
            id: format!("glyph:{}", self.data.glyphs.len()),
            source_record_id: r.id.clone(),
            context_node_id: self.data.nodes[n].id.clone(),
            source: r.source.clone(),
            name,
            definition_source: l.definition.clone(),
            program,
            interpretation_status: if reason.is_none() {
                Status::Complete
            } else {
                Status::Unsupported
            },
            reason: reason.map(str::to_owned),
        });
        self.last = None;
        Ok(())
    }
    pub(super) fn resolve_glyphs(&mut self, complete: bool) -> Result<(), HsfError> {
        if self.pending_glyphs.is_empty() {
            return Ok(());
        }
        self.budget
            .charge_output(self.data.glyphs.len() as u64 * 1024)?;
        let mut names = HashMap::new();
        for (i, d) in self.data.glyphs.iter().enumerate() {
            let b = self.doc.source_bytes(&d.name.source).expect("name");
            if b.is_ascii() {
                names
                    .entry(b.to_ascii_lowercase())
                    .and_modify(|v| *v = None)
                    .or_insert(Some(i));
            }
        }
        for p in std::mem::take(&mut self.pending_glyphs) {
            self.budget.charge_output(2048)?;
            let name = self.doc.source_bytes(&p.name).expect("name");
            let mut target = None;
            let reason = if !complete {
                Some("hsf.glyph_incomplete_scene")
            } else if !name_allowed(name) {
                Some("hsf.glyph_name")
            } else {
                match names.get(&name.to_ascii_lowercase()) {
                    None => Some("hsf.glyph_missing"),
                    Some(None) => Some("hsf.glyph_ambiguous"),
                    Some(Some(i)) => {
                        let d = &self.data.glyphs[*i];
                        if self.doc.source_bytes(&d.name.source).expect("name") != name {
                            Some("hsf.glyph_name_case")
                        } else {
                            target = Some(d.id.clone());
                            if d.context_node_id != self.data.nodes[self.import_node].id {
                                Some("hsf.glyph_scope")
                            } else if d.interpretation_status != Status::Complete {
                                Some("hsf.glyph_program")
                            } else {
                                None
                            }
                        }
                    }
                }
            };
            let binding = GlyphBinding {
                scope: "file_root_unique".into(),
                definition_id: target,
                status: if reason.is_none() {
                    Status::Complete
                } else {
                    Status::Unsupported
                },
                reason: reason.map(str::to_owned),
            };
            let event = &mut self.data.rendering_options[p.event];
            event.interpretation_status = binding.status.clone();
            event.glyph_binding = Some(binding.clone());
            let event_source = event.source.clone();
            // Only nodes that set a marker symbol own attributes to update; the
            // shared empty value is never copied for a lookup.
            if self.data.nodes[p.node].display.marker_symbol.is_some()
                && let Some(a) = self
                    .display_mut(p.node)?
                    .marker_symbol
                    .as_mut()
                    .filter(|a| a.source == event_source)
            {
                a.value.interpretation_status = binding.status.clone();
                a.value.symbol = reason.is_none().then(|| "custom".into());
                a.value.binding = Some(binding);
            }
            if let Some(reason) = reason {
                let mut e = error(
                    "hsf.rendering_semantics",
                    Status::Unsupported,
                    "Named marker glyph is unresolved; placements are withheld",
                );
                e.diagnostic.stage = "scene".into();
                e.diagnostic.source = Some(event_source);
                e.diagnostic.opcode = Some(b'@');
                e.diagnostic.version = Some(self.doc.header.version.text.clone());
                e.diagnostic.impact = Some(reason.into());
                if self.options.mode == ParseMode::Strict {
                    return Err(e);
                }
                self.budget.charge_output(512)?;
                self.diagnostics.push(*e.diagnostic);
            }
        }
        Ok(())
    }
}
fn decode(
    doc: &HsfDocument,
    source: &SourceSpan,
    budget: &mut Budget,
) -> Result<(Option<GlyphProgram>, Option<&'static str>), HsfError> {
    let data = doc.source_bytes(source).expect("glyph bytes");
    let bad = || {
        error(
            "hsf.invalid_glyph",
            Status::Invalid,
            "Truncated or unterminated glyph program",
        )
    };
    if data.len() < 4 {
        return Err(bad());
    }
    if data[0] == 0 || data[0] > 127 || data[1..3].contains(&128) {
        return Ok((None, Some("hsf.glyph_header")));
    }
    let offset = [i16::from(data[1] as i8), i16::from(data[2] as i8)];
    let mut at = 3;
    let mut points = 0;
    let mut commands = Vec::new();
    let mut reason = None;
    while at < data.len() {
        let start = at;
        let code = data[at] as i8;
        at += 1;
        if code == 0 {
            if at != data.len() {
                return Err(bad());
            }
            return Ok((
                Some(GlyphProgram {
                    nominal_radius: data[0],
                    offset,
                    header_source: local_span(source, 0, 3),
                    commands,
                    termination_source: local_span(source, start, 1),
                }),
                reason,
            ));
        }
        let (kind, count, colors) = match code {
            1 => ("dot", 1, 0),
            2..=127 => ("polyline", code as usize, 0),
            -1 => ("start_fill", 0, 0),
            -2 => ("restart_fill", 0, 0),
            -3 => ("end_fill", 0, 0),
            -4 => ("ellipse", 2, 0),
            -5 => ("circular_arc", 3, 0),
            -6 => ("infinite_line", 2, 0),
            -7 => ("infinite_ray", 2, 0),
            -8 => ("contrast_color", 0, 0),
            -9 => ("normal_color", 0, 0),
            -10 => ("rgb_color", 0, 3),
            -11 => ("indexed_color", 0, 1),
            _ => return Ok((None, Some("hsf.glyph_instruction"))),
        };
        if code < 0 {
            reason = Some("hsf.glyph_drawing_state");
        }
        let len = count * 2 + colors;
        if len > data.len() - at {
            return Err(bad());
        }
        points += count;
        limit(points as u64, 4096, "hsf.limit_glyph_points")?;
        limit(commands.len() as u64 + 1, 512, "hsf.limit_glyph_commands")?;
        budget.charge_output(1024 + count as u64 * 32)?;
        let payload = &data[at..at + len];
        if count != 0 && payload.contains(&128) {
            return Ok((None, Some("hsf.glyph_coordinate")));
        }
        commands.push(GlyphCommand {
            code: i16::from(code),
            kind: kind.into(),
            points: payload[..count * 2]
                .chunks_exact(2)
                .map(|p| [i16::from(p[0] as i8), i16::from(p[1] as i8)])
                .collect(),
            color: payload[count * 2..].to_vec(),
            source: local_span(source, start, 1 + len),
            payload_source: local_span(source, at, len),
        });
        at += len;
    }
    Err(bad())
}
