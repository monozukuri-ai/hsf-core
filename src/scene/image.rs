//! Owned image inventory and byte-level name correlations, not rendering.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct ImageDefinition {
    pub id: String,
    pub node_id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub profile: String,
    pub name: TextValue,
    /// Stored reference point, not a world placement or a pixel-size transform.
    pub position: Attribute<[f32; 3]>,
    pub format_options: u32,
    pub implicit_texture_local: bool,
    pub width: u32,
    pub height: u32,
    pub compression: u8,
    /// Encoded JPEG; no entropy decoding or pixel allocation is performed.
    pub data_source: SourceSpan,
    pub alpha_compression: u8,
    pub alpha_source: SourceSpan,
    pub interpretation_status: Status,
}

impl Builder<'_> {
    pub(super) fn image(&mut self, r: &Record, node: usize) -> Result<(), HsfError> {
        limit(
            self.definition_count() as u64 + 1,
            self.options.max_definitions,
            "hsf.limit_definitions",
        )?;
        self.budget.charge_output(2048)?;
        let layout = r.image.as_ref().expect("framed image");
        let name = primitive::text(self.doc, &layout.name, None, self.budget)?;
        let bytes = self
            .doc
            .source_bytes(&layout.position)
            .expect("framed point");
        let position = std::array::from_fn(|i| {
            f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("float"))
        });
        self.issue(r, "hsf.image_inventory", "Encoded image and owner retained; pixels, implicit texture effects and placements are not evaluated")?;
        self.diagnostics.last_mut().expect("issued").severity = "warning".into();
        let index = self.data.images.len();
        self.data.images.push(ImageDefinition {
            id: format!("image:{index}"),
            node_id: self.data.nodes[node].id.clone(),
            source_record_id: r.id.clone(),
            source: r.source.clone(),
            profile: layout.profile.clone(),
            name,
            position: Attribute {
                value: position,
                source: layout.position.clone(),
            },
            format_options: layout.format_options,
            implicit_texture_local: layout.format_options & 0x20 != 0,
            width: layout.width,
            height: layout.height,
            compression: layout.compression,
            data_source: layout.data.clone(),
            alpha_compression: layout.alpha_compression,
            alpha_source: layout.alpha.clone(),
            interpretation_status: Status::Unsupported,
        });
        // Image is the inserted object. A subsequent Tag must never attach to
        // preceding geometry. Images do not create geometry placement links.
        self.last = Some(Object::Image(index));
        Ok(())
    }

    pub(super) fn match_image_names(&mut self) -> Result<(), HsfError> {
        if self.data.images.is_empty() || self.data.textures.is_empty() {
            return Ok(());
        }
        self.budget
            .charge_output(self.data.images.len() as u64 * 512)?;
        let mut names: HashMap<&[u8], Vec<usize>> = HashMap::new();
        for (i, image) in self.data.images.iter().enumerate() {
            names
                .entry(
                    self.doc
                        .source_bytes(&image.name.source)
                        .expect("image name"),
                )
                .or_default()
                .push(i);
        }
        for texture in &mut self.data.textures {
            let name = self
                .doc
                .source_bytes(&texture.image.source)
                .expect("texture image name");
            if let Some(indices) = names.get(name) {
                for index in indices {
                    let id = &self.data.images[*index].id;
                    // Match multiplicity can be quadratic; charge before each
                    // allocation and never silently truncate ambiguous matches.
                    self.budget.charge_output(96 + id.len() as u64)?;
                    texture.image_name_matches.push(id.clone());
                }
            }
        }
        Ok(())
    }
}
