//! Numeric pattern attributes and deferred, bounded file-root custom bindings.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct LinePattern {
    pub binding: Option<LineStyleBinding>,
    /// Original packed pattern/cap/join word, absent for a named assignment.
    pub index: Option<u16>,
    pub name: Option<TextValue>,
    pub pattern: Option<String>,
    pub cap: Option<String>,
    pub join: Option<String>,
    pub interpretation_status: Status,
}
#[derive(Clone, Debug, Serialize)]
pub struct MarkerSymbol {
    pub binding: Option<GlyphBinding>,
    pub index: Option<u16>,
    pub name: Option<TextValue>,
    pub symbol: Option<String>,
    pub interpretation_status: Status,
}

const PATTERNS: [&str; 10] = [
    "solid",
    "dash_dot",
    "dashed",
    "dotted",
    "dash_dot_dot",
    "dash_dot_dot_dot",
    "long_dash",
    "center_line",
    "phantom_line",
    "fine_dotted",
];
const CAPS: [&str; 3] = ["none", "round", "square"];
const JOINS: [&str; 3] = ["miter", "round", "bevel"];
pub(super) const SYMBOLS: [&str; 49] = [
    "circle",
    "circle_dot",
    "circle_plus",
    "circle_times",
    "circle_circle",
    "filled_circle",
    "dot",
    "plus",
    "times",
    "star",
    "square",
    "square_dot",
    "square_times",
    "filled_square",
    "diamond",
    "diamond_dot",
    "diamond_plus",
    "filled_diamond",
    "triangle_up",
    "triangle_up_top",
    "triangle_up_dot",
    "filled_triangle_up",
    "filled_triangle_up_top",
    "triangle_down",
    "triangle_down_bottom",
    "triangle_down_dot",
    "filled_triangle_down",
    "filled_triangle_down_bottom",
    "triangle_right",
    "triangle_right_right",
    "triangle_right_dot",
    "filled_triangle_right",
    "filled_triangle_right_right",
    "triangle_left",
    "triangle_left_left",
    "triangle_left_dot",
    "filled_triangle_left",
    "filled_triangle_left_left",
    "hash",
    "wide_plus",
    "triangle_up_open_bottom",
    "triangle_up_tall",
    "triangle_up_wide",
    "two_hollow_triangles",
    "vertical_bar",
    "slash",
    "backslash",
    "y",
    "z",
];

impl Builder<'_> {
    pub(super) fn pattern_attribute(&mut self, r: &Record, n: usize) -> Result<(), HsfError> {
        // One local attribute plus its ordered event. Names have an existing u8
        // wire length; their decoding is separately charged by primitive::text.
        // Cloned display values remain within the existing 8192-byte allowance.
        self.budget.charge_output(4096)?;
        let layout = r.named_style.as_ref().expect("framed pattern");
        let name = layout
            .name
            .as_ref()
            .map(|s| primitive::text(self.doc, s, None, self.budget))
            .transpose()?;
        let status;
        if r.opcode == b'@' {
            let symbol = layout
                .index
                .and_then(|i| SYMBOLS.get(usize::from(i)))
                .map(|s| (*s).into());
            status = if symbol.is_some() {
                Status::Complete
            } else {
                Status::Unsupported
            };
            self.data.nodes[n].display.marker_symbol = Some(Attribute {
                source: r.source.clone(),
                value: MarkerSymbol {
                    binding: None,
                    index: layout.index,
                    name,
                    symbol,
                    interpretation_status: status.clone(),
                },
            });
        } else {
            let pattern = layout
                .index
                .and_then(|i| PATTERNS.get(usize::from(i & 255)))
                .map(|s| (*s).into());
            let cap = layout
                .index
                .and_then(|i| CAPS.get(usize::from((i >> 8) & 15)))
                .map(|s| (*s).into());
            let join = layout
                .index
                .and_then(|i| JOINS.get(usize::from(i >> 12)))
                .map(|s| (*s).into());
            status = if pattern.is_some() && cap.is_some() && join.is_some() {
                Status::Complete
            } else {
                Status::Unsupported
            };
            let attribute = Some(Attribute {
                source: r.source.clone(),
                value: LinePattern {
                    binding: None,
                    index: layout.index,
                    name,
                    pattern,
                    cap,
                    join,
                    interpretation_status: status.clone(),
                },
            });
            // The composite assignment replaces pattern, cap and join together.
            // Even an unknown value replaces old local state, preventing stale
            // known values from masquerading as the current assignment.
            if r.opcode == b'-' {
                self.data.nodes[n].display.line_pattern = attribute;
            } else {
                self.data.nodes[n].display.edge_pattern = attribute;
            }
        }
        let pending = layout.name.is_some();
        if pending && r.opcode == b'@' {
            self.pending_glyphs.push(glyph::PendingGlyph {
                node: n,
                event: self.data.rendering_options.len(),
                name: layout.name.clone().expect("name"),
            });
        } else if pending {
            self.pending_line_patterns
                .push(line_style::PendingLinePattern {
                    node: n,
                    event: self.data.rendering_options.len(),
                    name: layout.name.clone().expect("name"),
                });
        }
        if status != Status::Complete && !pending {
            self.issue(r, "hsf.rendering_semantics", "Named or unknown pattern effects are not evaluated; source values are retained and placements are withheld")?;
        }
        self.data.rendering_options.push(RenderingOptions {
            line_style_binding: None,
            glyph_binding: None,
            settings: None,
            node_id: self.data.nodes[n].id.clone(),
            source_record_id: r.id.clone(),
            source: r.source.clone(),
            kind: r.kind.clone(),
            interpretation_status: status,
        });
        Ok(())
    }
}
