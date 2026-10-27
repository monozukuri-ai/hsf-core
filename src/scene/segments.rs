//! Bounded literal paths and source-backed environment/style references.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct SegmentAlias {
    pub alias: String,
    pub target_path: String,
    /// First use of this assumed default binding, not a definition in the file.
    pub source_record_id: String,
    pub source: SourceSpan,
    pub interpretation_status: Status,
}

#[derive(Clone, Debug, Serialize)]
pub struct StyleSegmentReference {
    pub id: String,
    pub node_id: String,
    /// Existing target at the opcode; never backfilled from a later Open.
    pub target_id: Option<String>,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub path: TextValue,
    pub condition: Option<TextValue>,
    pub interpretation_status: Status,
}

impl Builder<'_> {
    pub(super) fn definition_count(&self) -> usize {
        self.data.images.len()
            + self.data.glyphs.len()
            + self.data.line_styles.len()
            + self.data.fonts.len()
            + self.data.textures.len()
            + self.data.style_segments.len()
            + self.data.segment_aliases.len()
    }

    pub(super) fn segment_path(
        &mut self,
        current: usize,
        name: &str,
        r: &Record,
    ) -> Result<Option<String>, HsfError> {
        // Quoted names, lists and wildcard searches require their own grammar.
        // Only space is a supported whitespace character in an unquoted name.
        if name.trim_matches(' ').is_empty()
            || !name.is_ascii()
            || name
                .bytes()
                .any(|b| !(b.is_ascii_alphanumeric() || b" #+-$_.^/?".contains(&b)))
        {
            return Ok(None);
        }
        // Account for temporary normalized components and the candidate path
        // before allocating. These are additional to retained node-path charges.
        let components = name.bytes().filter(|b| *b == b'/').count() as u64 + 1;
        let base_len = self.node_paths[current].as_ref().map_or(0, String::len) as u64;
        self.budget
            .charge_output(name.len() as u64 * 3 + components * 80 + base_len * 8 + 128)?;
        let normalized: Vec<String> = name
            .split('/')
            .map(|p| {
                let mut part = String::with_capacity(p.len());
                let mut space = false;
                for b in p.bytes() {
                    if b == b' ' {
                        space = !part.is_empty();
                    } else {
                        if space {
                            part.push(' ');
                        }
                        part.push(char::from(b.to_ascii_lowercase()));
                        space = false;
                    }
                }
                part
            })
            .collect();
        if normalized.iter().any(|p| p == "..." || p == "....") {
            return Ok(None);
        }
        let (binding, skip) = if normalized[0].starts_with('?') {
            let binding = match normalized[0].as_str() {
                "?home" => ("?Home", "/"),
                "?include library" => ("?Include Library", "/include library"),
                "?style library" => ("?Style Library", "/style library"),
                _ => return Ok(None),
            };
            (Some(binding), 1)
        } else {
            (None, 0)
        };
        if normalized[skip..]
            .iter()
            .any(|p| p.contains('?') || (p.contains('^') && p != "^"))
        {
            return Ok(None);
        }
        let base = if let Some((_, path)) = binding {
            path
        } else if name.starts_with('/') {
            "/"
        } else if let Some(path) = &self.node_paths[current] {
            path
        } else {
            return Ok(None);
        };
        let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
        for p in &normalized[skip..] {
            match p.as_str() {
                "" | "." => {}
                ".." | "^" => {
                    if parts.pop().is_none() {
                        return Ok(None);
                    }
                }
                _ => parts.push(p),
            }
        }
        let path = format!("/{}", parts.join("/"));
        if let Some((alias, target_path)) = binding {
            if !self.data.segment_aliases.iter().any(|a| a.alias == alias) {
                limit(
                    self.definition_count() as u64 + 1,
                    self.options.max_definitions,
                    "hsf.limit_definitions",
                )?;
                self.issue(r, "hsf.segment_alias_default", "Standard startup alias assumed for inventory; runtime overrides are unknown and all placements are withheld")?;
                self.diagnostics.last_mut().expect("issued").severity = "warning".into();
                self.budget.charge_output(2048)?;
                self.data.segment_aliases.push(SegmentAlias {
                    alias: alias.into(),
                    target_path: target_path.into(),
                    source_record_id: r.id.clone(),
                    source: r.source.clone(),
                    interpretation_status: Status::Unsupported,
                });
            }
        }
        Ok(Some(path))
    }

    pub(super) fn style_segment(&mut self, r: &Record, n: usize) -> Result<(), HsfError> {
        limit(
            self.definition_count() as u64 + 1,
            self.options.max_definitions,
            "hsf.limit_definitions",
        )?;
        self.budget.charge_output(2048)?;
        let path = primitive::text(
            self.doc,
            r.payload.as_ref().expect("framed path"),
            None,
            self.budget,
        )?;
        let target = if let Some(name) = &path.value {
            self.segment_path(n, name, r)?
                .and_then(|p| self.paths.get(&p).copied())
        } else {
            None
        };
        // Resolving an alias may itself consume a definition slot.
        limit(
            self.definition_count() as u64 + 1,
            self.options.max_definitions,
            "hsf.limit_definitions",
        )?;
        let condition = r
            .condition
            .as_ref()
            .map(|s| primitive::text(self.doc, s, None, self.budget))
            .transpose()?;
        self.style_edges[n].push((self.data.style_segments.len(), target));
        self.data.style_segments.push(StyleSegmentReference {
            id: format!("style_segment:{}", self.data.style_segments.len()),
            node_id: self.data.nodes[n].id.clone(),
            target_id: target.map(|i| self.data.nodes[i].id.clone()),
            source_record_id: r.id.clone(),
            source: r.source.clone(),
            path,
            condition,
            interpretation_status: Status::Unsupported,
        });
        self.last = None;
        Ok(())
    }
}
