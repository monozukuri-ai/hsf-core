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

/// A literal path resolved against the file tree, or a runtime path that leaves
/// the mounted file root and therefore names nothing in this file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ResolvedPath {
    Internal(String),
    External(String),
}

/// Stored spelling of a literal component: plain when the unquoted grammar
/// would read it back identically, otherwise wrapped in backquotes.
fn quote_component(p: &str) -> String {
    let plain = !p.is_empty()
        && !p.starts_with(' ')
        && !p.ends_with(' ')
        && !p.contains("  ")
        && !matches!(p, "." | ".." | "..." | "....")
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" #+-$_.".contains(&b));
    if plain {
        p.to_owned()
    } else {
        format!("`{p}`")
    }
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
    ) -> Result<Option<ResolvedPath>, HsfError> {
        // Lists and wildcard searches require their own grammar. Only space is a
        // supported whitespace character in an unquoted name. A component wrapped
        // in backquotes is a literal name (eDrawings 2026 opens "`eDrawings`"):
        // the quotes are not part of the name, navigation/alias/wildcard meanings
        // do not apply, and any printable ASCII except '`' and '/' is kept as is.
        // Text glued to a quote, nested quotes and unbalanced quotes stay unsupported.
        if name.trim_matches(' ').is_empty() || !name.is_ascii() {
            return Ok(None);
        }
        let mut raw_parts: Vec<(String, bool)> = vec![(String::new(), false)];
        let mut in_quote = false;
        for b in name.bytes() {
            let last = raw_parts.last_mut().expect("one part");
            if in_quote {
                match b {
                    b'`' => in_quote = false,
                    b'/' => return Ok(None),
                    0x20..=0x7e => last.0.push(char::from(b)),
                    _ => return Ok(None),
                }
            } else {
                match b {
                    b'`' => {
                        if !last.0.is_empty() || last.1 {
                            return Ok(None);
                        }
                        in_quote = true;
                        last.1 = true;
                    }
                    b'/' => raw_parts.push((String::new(), false)),
                    _ if b.is_ascii_alphanumeric() || b" #+-$_.^?".contains(&b) => {
                        if last.1 {
                            return Ok(None);
                        }
                        last.0.push(char::from(b));
                    }
                    _ => return Ok(None),
                }
            }
        }
        if in_quote || raw_parts.iter().any(|(p, quoted)| *quoted && p.is_empty()) {
            return Ok(None);
        }
        // Account for temporary normalized components and the candidate path
        // before allocating. These are additional to retained node-path charges.
        let components = name.bytes().filter(|b| *b == b'/').count() as u64 + 1;
        let base_len = self.node_paths[current].as_ref().map_or(0, String::len) as u64;
        let mount_len: u64 = self
            .mount
            .as_ref()
            .map_or(0, |m| m.iter().map(|c| c.len() as u64 + 1).sum());
        self.budget.charge_output(
            name.len() as u64 * 3 + components * 80 + (base_len + mount_len) * 8 + 128,
        )?;
        // Names are case-insensitive. Unquoted components collapse and trim spaces;
        // quoted components keep their spacing and are stored re-quoted whenever
        // the plain spelling would read as navigation or special syntax.
        let normalized: Vec<(String, bool)> = raw_parts
            .into_iter()
            .map(|(p, quoted)| {
                if quoted {
                    return (quote_component(&p.to_ascii_lowercase()), true);
                }
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
                (part, false)
            })
            .collect();
        if normalized
            .iter()
            .any(|(p, quoted)| !quoted && (p == "..." || p == "...."))
        {
            return Ok(None);
        }
        let (binding, skip) = if !normalized[0].1 && normalized[0].0.starts_with('?') {
            let binding = match normalized[0].0.as_str() {
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
            .any(|(p, quoted)| !quoted && (p.contains('?') || (p.contains('^') && p != "^")))
        {
            return Ok(None);
        }
        let base: String = if let Some((_, path)) = binding {
            path.to_owned()
        } else if name.starts_with('/') {
            "/".to_owned()
        } else if let Some(path) = &self.node_paths[current] {
            path.clone()
        } else {
            return Ok(None);
        };
        let mut parts: Vec<String> = base
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let mount = self.mount.clone();
        let mut escaped = false;
        for (p, quoted) in &normalized[skip..] {
            if *quoted {
                parts.push(p.clone());
                continue;
            }
            match p.as_str() {
                "" | "." => {}
                ".." | "^" => {
                    if parts.pop().is_none() {
                        // Above the file root. The producer's runtime continues
                        // through the mount path's ancestors; without a mount, or
                        // above the runtime root, the path stays unsupported.
                        let Some(mount) = mount.as_ref().filter(|_| !escaped) else {
                            return Ok(None);
                        };
                        let mut above = mount.clone();
                        if above.pop().is_none() {
                            return Ok(None);
                        }
                        parts = above;
                        escaped = true;
                    }
                }
                _ => parts.push(p.clone()),
            }
        }
        let path = if escaped {
            let mount = mount.as_ref().expect("escaped paths have a mount");
            if parts.len() >= mount.len() && parts.iter().zip(mount).all(|(p, m)| p == m) {
                format!("/{}", parts[mount.len()..].join("/"))
            } else {
                return Ok(Some(ResolvedPath::External(format!(
                    "/{}",
                    parts.join("/")
                ))));
            }
        } else {
            format!("/{}", parts.join("/"))
        };
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
        Ok(Some(ResolvedPath::Internal(path)))
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
            self.segment_path(n, name, r)?.and_then(|p| match p {
                ResolvedPath::Internal(p) => self.paths.get(&p).copied(),
                ResolvedPath::External(_) => None,
            })
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
