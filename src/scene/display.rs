use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct MarkerSize {
    pub value: f32,
    /// relative, object, screen, window, points, or pixels; no model-unit conversion.
    pub units: String,
}

/// Ordered definition events. Names do not select an active line pattern.
#[derive(Clone, Debug, Serialize)]
pub struct LineStyleDefinition {
    pub program: Option<LineStyleProgram>,
    pub reason: Option<String>,
    pub id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub context_node_id: String,
    pub name: TextValue,
    pub definition: TextValue,
    pub interpretation_status: Status,
}

/// Font bytes and lookup identifiers are retained, never executed or loaded.
#[derive(Clone, Debug, Serialize)]
pub struct FontDefinition {
    pub id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub context_node_id: String,
    pub format: u8,
    pub name: TextValue,
    pub encoding: u8,
    pub lookup: TextValue,
    pub data_source: SourceSpan,
    pub interpretation_status: Status,
}

impl Builder<'_> {
    pub(super) fn definition(&mut self, r: &Record, n: usize) -> Result<(), HsfError> {
        limit(
            self.definition_count() as u64 + 1,
            self.options.max_definitions,
            "hsf.limit_definitions",
        )?;
        self.budget.charge_output(2048)?;
        if let Some(l) = &r.line_style {
            let name = primitive::text(self.doc, &l.name, None, self.budget)?;
            let definition = primitive::text(self.doc, &l.definition, None, self.budget)?;
            let program = line_style::decode_program(self.doc, &l.definition, self.budget)?;
            let reason = if !line_style::custom_name(self.doc.source_bytes(&l.name).expect("name"))
            {
                Some("hsf.line_style_name".to_owned())
            } else {
                match program.as_ref() {
                    None => Some("hsf.line_style_program".to_owned()),
                    Some(p) if p.interpretation_status != Status::Complete => p.reason.clone(),
                    Some(_) => None,
                }
            };
            if reason.is_some() {
                self.issue(r, "hsf.line_style_definition", "Line style definition is retained; unknown instructions, omitted units or unresolved geometry options are not interpreted")?;
            }
            self.data.line_styles.push(LineStyleDefinition {
                program,
                reason: reason.clone(),
                id: format!("line_style:{}", self.data.line_styles.len()),
                source_record_id: r.id.clone(),
                source: r.source.clone(),
                context_node_id: self.data.nodes[n].id.clone(),
                name,
                definition,
                interpretation_status: if reason.is_none() {
                    Status::Complete
                } else {
                    Status::Unsupported
                },
            });
        } else {
            let l = r.font.as_ref().expect("framed font");
            let name = primitive::text(self.doc, &l.name, None, self.budget)?;
            let lookup = primitive::text(self.doc, &l.lookup, None, self.budget)?;
            self.issue(
                r,
                "hsf.font_definition",
                "Font definition is retained; glyph data and lookup functions are not interpreted",
            )?;
            self.data.fonts.push(FontDefinition {
                id: format!("font:{}", self.data.fonts.len()),
                source_record_id: r.id.clone(),
                source: r.source.clone(),
                context_node_id: self.data.nodes[n].id.clone(),
                format: l.format,
                name,
                encoding: l.encoding,
                lookup,
                data_source: l.data.clone(),
                interpretation_status: Status::Unsupported,
            });
        }
        self.last = None;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct NullShell {
    /// The unique supported LOD 0 geometry, or None for an empty/conflicting object.
    pub mesh_id: Option<String>,
    pub id: String,
    pub node_id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub lod: u8,
}

/// Ordered settings, linked to framed operands. No effective rendering is claimed.
#[derive(Clone, Debug, Serialize)]
pub struct RenderingOptions {
    pub glyph_binding: Option<GlyphBinding>,
    pub settings: Option<RenderingSettings>,
    pub line_style_binding: Option<LineStyleBinding>,
    pub node_id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub kind: String,
    pub interpretation_status: Status,
}

/// Segment metadata; signed arrays remain source-backed. No application IDs inferred.
#[derive(Clone, Debug, Serialize)]
pub struct UserIndices {
    pub node_id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub layout: UserIndexLayout,
    pub interpretation_status: Status,
}

/// Named image references only: never load external files, URLs or texture code.
#[derive(Clone, Debug, Serialize)]
pub struct TextureDefinition {
    pub options: TextureOptions,
    pub id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub context_node_id: String,
    pub name: TextValue,
    pub image: TextValue,
    /// Byte-identical names in the consumed scene prefix, including later images.
    /// These are candidates only: no scope, time, override or rendering binding.
    pub image_name_matches: Vec<String>,
    pub flags: u32,
    pub interpretation_status: Status,
}
