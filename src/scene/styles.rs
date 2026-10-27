//! Final local-plus-style display values, followed by ordinary instance inheritance.
//! A style imports local attributes (including its own styles), never its parent,
//! geometry or child segments. Unknown rules invalidate the whole resolution.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct StyleResolution {
    pub node_id: String,
    pub style_ids: Vec<String>,
    /// Local settings over unambiguous unconditional styles. No parent values,
    /// runtime defaults, locks, texture binding, projection or external state.
    /// None when any required rule is unsupported; never a guessed partial set.
    pub display: Option<DisplayAttributes>,
    pub status: Status,
    pub reason: Option<String>,
}

struct Resolved {
    display: Option<Box<DisplayAttributes>>,
    reason: Option<&'static str>,
    depth: u64,
}
struct Frame {
    node: usize,
    next: usize,
    styles: DisplayAttributes,
    reason: Option<&'static str>,
    depth: u64,
}

fn overlaps(a: &DisplayAttributes, b: &DisplayAttributes) -> bool {
    (a.text_anchor.is_some() && b.text_anchor.is_some())
        || (a.text_justification.is_some() && b.text_justification.is_some())
        || (a.line_pattern.is_some() && b.line_pattern.is_some())
        || (a.edge_pattern.is_some() && b.edge_pattern.is_some())
        || (a.marker_symbol.is_some() && b.marker_symbol.is_some())
        || a.colors
            .iter()
            .zip(&b.colors)
            .any(|(a, b)| a.is_some() && b.is_some())
        || a.visibility
            .iter()
            .zip(&b.visibility)
            .any(|(a, b)| a.is_some() && b.is_some())
        || (a.marker_size.is_some() && b.marker_size.is_some())
        || (a.line_weight.is_some() && b.line_weight.is_some())
        || (a.edge_weight.is_some() && b.edge_weight.is_some())
        || (a.texture_matrix.is_some() && b.texture_matrix.is_some())
}

impl Builder<'_> {
    pub(super) fn resolve_styles(&mut self, complete_prefix: bool) -> Result<(), HsfError> {
        self.resolve_styles_inner(complete_prefix).map_err(|mut e| {
            if e.diagnostic.stage == "hsf_header" {
                e.diagnostic.stage = "scene".into();
            }
            e.diagnostic.version = Some(self.doc.header.version.text.clone());
            e
        })
    }

    fn resolve_styles_inner(&mut self, complete_prefix: bool) -> Result<(), HsfError> {
        if self.data.style_segments.is_empty() {
            return Ok(());
        }
        let count = self.data.nodes.len();
        // IDs/index, flags, cache handles and frames; each actual display clone
        // is charged separately. No display-sized allocation for every node.
        self.budget.charge_output(count as u64 * 512)?;
        let by_id: HashMap<&str, usize> = self
            .data
            .nodes
            .iter()
            .enumerate()
            .map(|(n, node)| (node.id.as_str(), n))
            .collect();
        let mut opaque = vec![false; count];
        for id in self
            .data
            .rendering_options
            .iter()
            .filter(|x| x.interpretation_status != Status::Complete)
            .map(|x| &x.node_id)
            .chain(self.data.textures.iter().map(|x| &x.context_node_id))
            .chain(self.data.images.iter().map(|x| &x.node_id))
            .chain(self.data.cameras.iter().map(|x| &x.node_id))
        {
            opaque[by_id[id.as_str()]] = true;
        }
        let mut state = vec![0u8; count];
        let mut cache: Vec<Option<Resolved>> = (0..count).map(|_| None).collect();
        let mut stack: Vec<Frame> = Vec::new();
        for start in 0..count {
            if state[start] != 0 || self.style_edges[start].is_empty() {
                continue;
            }
            self.push_style_frame(start, &opaque, complete_prefix, &mut state, &mut stack)?;
            while let Some(frame) = stack.last_mut() {
                let n = frame.node;
                if frame.next == self.style_edges[n].len() {
                    let frame = stack.pop().expect("current frame");
                    let display = if frame.reason.is_none() {
                        self.budget.charge_output(8192)?;
                        Some(Box::new(self.data.nodes[n].display.inherit(&frame.styles)))
                    } else {
                        None
                    };
                    cache[n] = Some(Resolved {
                        display,
                        reason: frame.reason,
                        depth: frame.depth,
                    });
                    state[n] = 2;
                    continue;
                }
                let (s, target) = self.style_edges[n][frame.next];
                // Count a dependency once, not again when returning from it.
                self.visits += 1;
                limit(
                    self.visits,
                    self.options.max_visits,
                    "hsf.limit_scene_visits",
                )?;
                let style = &self.data.style_segments[s];
                let reason = if !complete_prefix {
                    Some("hsf.style_incomplete_scene")
                } else if style.condition.is_some() {
                    Some("hsf.style_condition")
                } else if target.is_none() {
                    Some("hsf.style_unresolved")
                } else {
                    let t = target.expect("checked");
                    let node = &self.data.nodes[t];
                    if opaque[t]
                        || node.local_transform.is_some()
                        || node.world_handedness.is_some()
                    {
                        Some("hsf.style_target_attributes")
                    } else if state[t] == 1 {
                        Some("hsf.style_cycle")
                    } else {
                        None
                    }
                };
                if let Some(reason) = reason {
                    frame.reason.get_or_insert(reason);
                    frame.next += 1;
                    continue;
                }
                let t = target.expect("supported target");
                if state[t] == 0 {
                    // Charge this edge on completion, sharing visits with geometry.
                    self.visits -= 1;
                    self.push_style_frame(t, &opaque, complete_prefix, &mut state, &mut stack)?;
                    continue;
                }
                let resolved = cache[t].as_ref().expect("finished dependency");
                frame.depth = frame.depth.max(resolved.depth + 1);
                limit(frame.depth, self.options.max_depth, "hsf.limit_scene_depth")?;
                if let Some(display) = &resolved.display {
                    if overlaps(&frame.styles, display) {
                        frame.reason.get_or_insert("hsf.style_conflict");
                    } else {
                        self.budget.charge_output(8192)?;
                        frame.styles = display.inherit(&frame.styles);
                    }
                } else {
                    frame
                        .reason
                        .get_or_insert(resolved.reason.unwrap_or("hsf.style_dependency"));
                }
                frame.next += 1;
            }
        }
        // One deterministic result per styled node; local Node.display stays raw.
        for (n, edges) in self.style_edges.iter().enumerate() {
            if edges.is_empty() {
                continue;
            }
            let resolved = cache[n].take().expect("resolved styled node");
            self.budget
                .charge_output(16384 + edges.len() as u64 * 128)?;
            let status = if resolved.reason.is_none() {
                Status::Complete
            } else {
                Status::Unsupported
            };
            let style_ids = edges
                .iter()
                .map(|(s, _)| self.data.style_segments[*s].id.clone())
                .collect();
            self.data.style_resolutions.push(StyleResolution {
                node_id: self.data.nodes[n].id.clone(),
                style_ids,
                display: resolved.display.as_deref().cloned(),
                status: status.clone(),
                reason: resolved.reason.map(str::to_owned),
            });
            self.styled_displays[n] = resolved.display;
            for (s, _) in edges {
                self.data.style_segments[*s].interpretation_status = status.clone();
            }
            if let Some(reason) = resolved.reason {
                let style = &self.data.style_segments[edges[0].0];
                let mut e = error(
                    reason,
                    Status::Unsupported,
                    "Style display resolution is unsupported; local attributes and links are retained, all placements are withheld",
                );
                e.diagnostic.stage = "scene".into();
                e.diagnostic.source = Some(style.source.clone());
                e.diagnostic.opcode = Some(b'{');
                e.diagnostic.version = Some(self.doc.header.version.text.clone());
                if self.options.mode == ParseMode::Strict {
                    return Err(e);
                }
                e.diagnostic.severity = "warning".into();
                self.budget.charge_output(1024)?;
                self.diagnostics.push(*e.diagnostic);
            }
        }
        Ok(())
    }

    fn push_style_frame(
        &mut self,
        n: usize,
        opaque: &[bool],
        complete_prefix: bool,
        state: &mut [u8],
        stack: &mut Vec<Frame>,
    ) -> Result<(), HsfError> {
        limit(
            stack.len() as u64 + 1,
            self.options.max_depth,
            "hsf.limit_scene_depth",
        )?;
        self.budget.charge_output(8192)?;
        state[n] = 1;
        stack.push(Frame {
            node: n,
            next: 0,
            styles: DisplayAttributes::default(),
            depth: 1,
            reason: if !complete_prefix {
                Some("hsf.style_incomplete_scene")
            } else if opaque[n] {
                Some("hsf.style_target_attributes")
            } else {
                None
            },
        });
        Ok(())
    }
}
