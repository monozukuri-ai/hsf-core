//! Bounded custom-line programs and final, unique definitions at the file root.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct LineStyleProgram {
    /// solid, explicit_dash_blank, parallel_dash_blank, or extended_line_style.
    pub kind: String,
    /// Contract 26 simple form. Empty for parallel_dash_blank.
    pub elements: Vec<LineStyleElement>,
    pub parallels: Vec<LineStyleParallel>,
    /// Global 3D-space request. Geometry Options orientation is not resolved.
    pub transformed_source: Option<SourceSpan>,
    pub interpretation_status: Status,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct LineStyleElement {
    pub kind: String,
    /// None means an explicitly unsized, stretchable dash/blank.
    pub length: Option<f64>,
    pub units: Option<String>,
    pub stretchable: bool,
    pub source: SourceSpan,
    pub length_source: Option<SourceSpan>,
    pub units_source: Option<SourceSpan>,
}
#[derive(Clone, Debug, Serialize)]
pub struct LineStyleParallel {
    pub source: SourceSpan,
    /// None for the implicit first parallel.
    pub marker_source: Option<SourceSpan>,
    pub elements: Vec<LineStyleElement>,
    pub weight: Option<LineStyleMeasure>,
    pub offset: Option<LineStyleMeasure>,
    pub segmented_source: Option<SourceSpan>,
    pub fit: Option<LineStyleFit>,
    pub absolute_length_source: Option<SourceSpan>,
    pub absolute_weight_source: Option<SourceSpan>,
}
#[derive(Clone, Debug, Serialize)]
pub struct LineStyleFit {
    pub mode: String,
    pub source: SourceSpan,
    pub value_source: SourceSpan,
}
#[derive(Clone, Debug, Serialize)]
pub struct LineStyleMeasure {
    pub value: f64,
    /// None preserves an omitted weight unit; interpretation stays unsupported.
    pub units: Option<String>,
    pub fixed: bool,
    pub source: SourceSpan,
    pub value_source: SourceSpan,
    pub units_source: Option<SourceSpan>,
    pub fixed_source: Option<SourceSpan>,
}
#[derive(Clone, Debug, Serialize)]
pub struct LineStyleBinding {
    pub scope: String,
    pub definition_id: Option<String>,
    pub status: Status,
    pub reason: Option<String>,
}
pub(super) struct PendingLinePattern {
    pub node: usize,
    pub event: usize,
    pub name: SourceSpan,
}

// Only literal ASCII custom identifiers. Built-in aliases and names containing
// pattern cap/join syntax are excluded instead of interpreting the name string.
pub(super) fn custom_name(bytes: &[u8]) -> bool {
    let Some(first) = bytes.first() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && *first != b'_' {
        return false;
    }
    if bytes.last() == Some(&b' ')
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"_- ./".contains(b))
    {
        return false;
    }
    ![
        "solid",
        "dashed",
        "dotted",
        "dash",
        "dot",
        "center",
        "phantom",
        "finedot",
        "center_line",
        "phantom_line",
        "long_dash",
        "fine_dotted",
        "dash_dot",
        "dash_dot_dot",
        "dash_dot_dot_dot",
    ]
    .iter()
    .any(|s| bytes.eq_ignore_ascii_case(s.as_bytes()))
}

fn span(source: &SourceSpan, text: &str, part: &str) -> SourceSpan {
    SourceSpan {
        domain_id: source.domain_id.clone(),
        offset: source.offset + (part.as_ptr() as usize - text.as_ptr() as usize) as u64,
        length: part.len() as u64,
    }
}
fn units(token: &str) -> Option<&'static str> {
    match token {
        "oru" => Some("object_relative"),
        "px" | "pixel" | "pixels" => Some("pixels"),
        _ => None,
    }
}
fn empty_parallel(source: SourceSpan, marker_source: Option<SourceSpan>) -> LineStyleParallel {
    LineStyleParallel {
        source,
        marker_source,
        elements: vec![],
        weight: None,
        offset: None,
        segmented_source: None,
        fit: None,
        absolute_length_source: None,
        absolute_weight_source: None,
    }
}

pub(super) fn decode_program(
    doc: &HsfDocument,
    source: &SourceSpan,
    budget: &mut Budget,
) -> Result<Option<LineStyleProgram>, HsfError> {
    let bytes = doc.source_bytes(source).expect("definition span");
    if !bytes.is_ascii()
        || bytes
            .iter()
            .any(|b| b.is_ascii_control() && !b"\t\r\n".contains(b))
    {
        return Ok(None);
    }
    let text = std::str::from_utf8(bytes).expect("ASCII");
    let trimmed = text.trim_matches(|c: char| c.is_ascii_whitespace());
    if trimmed.is_empty() || trimmed == "dash" {
        budget.charge_output(1024)?;
        return Ok(Some(LineStyleProgram {
            kind: "solid".into(),
            elements: vec![],
            parallels: vec![],
            transformed_source: None,
            interpretation_status: Status::Complete,
            reason: None,
        }));
    }
    // Includes modifiers and delimiters; bounded before building any AST vectors.
    let count = text.split(',').count();
    limit(count as u64, 256, "hsf.limit_line_style_elements")?;
    budget.charge_output(1024 + count as u64 * 2048)?;
    let mut parallels = vec![empty_parallel(span(source, text, trimmed), None)];
    let mut unresolved_units = false;
    let mut transformed_source = None;
    for (index, raw) in text.split(',').enumerate() {
        let clause = raw.trim_matches(|c: char| c.is_ascii_whitespace());
        let clause_source = span(source, text, clause);
        let tokens: Vec<_> = clause.split_ascii_whitespace().take(5).collect();
        if tokens.as_slice() == ["transformed"] {
            // This is a global suffix, outside the last parallel's source span.
            if index + 1 != count || parallels.last().expect("parallel").elements.is_empty() {
                return Ok(None);
            }
            transformed_source = Some(clause_source);
            continue;
        }
        if tokens.as_slice() == ["parallel"] {
            if parallels.last().expect("parallel").elements.is_empty() {
                return Ok(None);
            }
            limit(
                parallels.len() as u64 + 1,
                32,
                "hsf.limit_line_style_parallels",
            )?;
            parallels.push(empty_parallel(clause_source.clone(), Some(clause_source)));
            continue;
        }
        let parallel = parallels.last_mut().expect("parallel");
        parallel.source.length =
            clause_source.offset + clause_source.length - parallel.source.offset;
        if let Some((key, value)) = clause.split_once('=') {
            let mode = value.trim_matches(|c: char| c.is_ascii_whitespace());
            if key.trim_matches(|c: char| c.is_ascii_whitespace()) != "fit"
                || !matches!(mode, "center" | "stretch")
                || parallel.fit.is_some()
                || !parallel.elements.is_empty()
            {
                return Ok(None);
            }
            parallel.fit = Some(LineStyleFit {
                mode: mode.into(),
                source: clause_source,
                value_source: span(source, text, mode),
            });
            continue;
        }
        match tokens.as_slice() {
            ["absolute"] | ["absolute", "length" | "weight"] => {
                if !parallel.elements.is_empty() {
                    return Ok(None);
                }
                let length = tokens.len() == 1 || tokens[1] == "length";
                let weight = tokens.len() == 1 || tokens[1] == "weight";
                if (length && parallel.absolute_length_source.is_some())
                    || (weight && parallel.absolute_weight_source.is_some())
                {
                    return Ok(None);
                }
                if length {
                    parallel.absolute_length_source = Some(clause_source.clone());
                }
                if weight {
                    parallel.absolute_weight_source = Some(clause_source);
                }
            }
            ["segmented"] => {
                if parallel.segmented_source.is_some() {
                    return Ok(None);
                }
                parallel.segmented_source = Some(clause_source);
            }
            [kind @ ("dash" | "blank" | "contrast")] => {
                parallel.elements.push(LineStyleElement {
                    kind: (*kind).into(),
                    length: None,
                    units: None,
                    stretchable: true,
                    source: clause_source,
                    length_source: None,
                    units_source: None,
                });
            }
            [number, unit, kind @ ("dash" | "blank" | "contrast")] => {
                let (Ok(length), Some(units)) = (number.parse::<f64>(), units(unit)) else {
                    return Ok(None);
                };
                if !length.is_finite() || length <= 0. {
                    return Ok(None);
                }
                parallel.elements.push(LineStyleElement {
                    kind: (*kind).into(),
                    length: Some(length),
                    units: Some(units.into()),
                    stretchable: false,
                    source: clause_source,
                    length_source: Some(span(source, text, number)),
                    units_source: Some(span(source, text, unit)),
                });
            }
            _ => {
                // Modifiers must precede elements. Duplicate assignments are not last-wins.
                if !parallel.elements.is_empty() {
                    return Ok(None);
                }
                let (number, unit, fixed, kind) = match tokens.as_slice() {
                    [n, "weight"] => (*n, None, None, "weight"),
                    [n, u, k @ ("weight" | "offset")] => (*n, Some(*u), None, *k),
                    [n, u, f @ "fixed", k @ ("weight" | "offset")] => (*n, Some(*u), Some(*f), *k),
                    _ => return Ok(None),
                };
                let Ok(value) = number.parse::<f64>() else {
                    return Ok(None);
                };
                if !value.is_finite() || (kind == "weight" && value <= 0.) {
                    return Ok(None);
                }
                let parsed_units = match unit {
                    Some(u) => match units(u) {
                        Some(v) => Some(v.to_owned()),
                        None => return Ok(None),
                    },
                    None => {
                        unresolved_units = true;
                        None
                    }
                };
                let target = if kind == "weight" {
                    &mut parallel.weight
                } else {
                    &mut parallel.offset
                };
                if target.is_some() {
                    return Ok(None);
                }
                // The documented profile orders offset before weight.
                if kind == "offset" && parallel.weight.is_some() {
                    return Ok(None);
                }
                let measure = LineStyleMeasure {
                    value,
                    units: parsed_units,
                    fixed: fixed.is_some(),
                    source: clause_source,
                    value_source: span(source, text, number),
                    units_source: unit.map(|u| span(source, text, u)),
                    fixed_source: fixed.map(|f| span(source, text, f)),
                };
                if kind == "weight" {
                    parallel.weight = Some(measure);
                } else {
                    parallel.offset = Some(measure);
                }
            }
        }
    }
    if parallels.iter().any(|p| p.elements.is_empty()) {
        return Ok(None);
    }
    let extended = transformed_source.is_some()
        || parallels.iter().any(|p| {
            p.fit.is_some()
                || p.absolute_length_source.is_some()
                || p.absolute_weight_source.is_some()
                || p.elements.iter().any(|e| e.kind == "contrast")
        });
    let simple = !extended
        && parallels.len() == 1
        && parallels[0].weight.is_none()
        && parallels[0].offset.is_none()
        && parallels[0].segmented_source.is_none()
        && parallels[0].elements.iter().all(|e| !e.stretchable);
    let elements = if simple {
        parallels.pop().expect("parallel").elements
    } else {
        vec![]
    };
    let reason = if unresolved_units {
        Some("hsf.line_style_weight_units")
    } else if transformed_source.is_some() {
        Some("hsf.line_style_geometry_options")
    } else {
        None
    };
    Ok(Some(LineStyleProgram {
        kind: if extended {
            "extended_line_style"
        } else if simple {
            "explicit_dash_blank"
        } else {
            "parallel_dash_blank"
        }
        .into(),
        elements,
        parallels,
        transformed_source,
        interpretation_status: if reason.is_some() {
            Status::Unsupported
        } else {
            Status::Complete
        },
        reason: reason.map(str::to_owned),
    }))
}

impl Builder<'_> {
    pub(super) fn resolve_line_patterns(&mut self, complete: bool) -> Result<(), HsfError> {
        if self.pending_line_patterns.is_empty() {
            return Ok(());
        }
        // Case-folding is only a collision guard. Actual matching is byte exact.
        self.budget
            .charge_output(self.data.line_styles.len() as u64 * 1024)?;
        let mut definitions: HashMap<Vec<u8>, Option<usize>> = HashMap::new();
        for (i, definition) in self.data.line_styles.iter().enumerate() {
            let name = self
                .doc
                .source_bytes(&definition.name.source)
                .expect("name");
            if !name.is_ascii() {
                continue;
            }
            definitions
                .entry(name.to_ascii_lowercase())
                .and_modify(|v| *v = None)
                .or_insert(Some(i));
        }
        for pending in std::mem::take(&mut self.pending_line_patterns) {
            self.budget.charge_output(2048)?;
            let name = self.doc.source_bytes(&pending.name).expect("name");
            let mut target = None;
            let reason = if !complete {
                Some("hsf.line_style_incomplete_scene")
            } else if !custom_name(name) {
                Some("hsf.line_style_name")
            } else {
                match definitions.get(&name.to_ascii_lowercase()) {
                    None => Some("hsf.line_style_missing"),
                    Some(None) => Some("hsf.line_style_ambiguous"),
                    Some(Some(i)) => {
                        let definition = &self.data.line_styles[*i];
                        if self
                            .doc
                            .source_bytes(&definition.name.source)
                            .expect("name")
                            != name
                        {
                            Some("hsf.line_style_name_case")
                        } else {
                            target = Some(definition.id.clone());
                            if definition.context_node_id != self.data.nodes[self.import_node].id {
                                Some("hsf.line_style_scope")
                            } else if definition.interpretation_status != Status::Complete {
                                Some("hsf.line_style_program")
                            } else {
                                None
                            }
                        }
                    }
                }
            };
            let binding = LineStyleBinding {
                scope: "file_root_unique".into(),
                definition_id: target,
                status: if reason.is_none() {
                    Status::Complete
                } else {
                    Status::Unsupported
                },
                reason: reason.map(str::to_owned),
            };
            let event = &mut self.data.rendering_options[pending.event];
            event.interpretation_status = binding.status.clone();
            event.line_style_binding = Some(binding.clone());
            let attr = if event.kind == "line_pattern" {
                &mut self.data.nodes[pending.node].display.line_pattern
            } else {
                &mut self.data.nodes[pending.node].display.edge_pattern
            };
            if let Some(attr) = attr.as_mut().filter(|a| a.source == event.source) {
                attr.value.pattern = reason.is_none().then(|| "custom".into());
                attr.value.interpretation_status = binding.status.clone();
                attr.value.binding = Some(binding);
            }
            if let Some(reason) = reason {
                let mut e = error(
                    "hsf.rendering_semantics",
                    Status::Unsupported,
                    "Named pattern effects are not evaluated under the unique file-root profile; source values are retained and placements are withheld",
                );
                e.diagnostic.stage = "scene".into();
                e.diagnostic.source = Some(event.source.clone());
                e.diagnostic.opcode = Some(if event.kind == "line_pattern" {
                    b'-'
                } else {
                    13
                });
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
