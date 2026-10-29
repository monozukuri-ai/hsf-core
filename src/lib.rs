//! Independent, bounded HSF syntax and generic scene reader.
mod scene;
mod syntax;
pub use scene::*;
use serde::{Deserialize, Serialize};
use std::fmt;
pub use syntax::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SOURCE_SHA256: &str = env!("HSF_SOURCE_SHA256");
pub const CONTRACT_VERSION: u32 = 34;
pub const MAX_HEADER_BYTES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub version: Version,
    /// Includes the terminating LF (and CR when present).
    pub byte_len: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NotRequested,
    Complete,
    Partial,
    Unsupported,
    Invalid,
    LimitExceeded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub domain_id: String,
    pub offset: u64,
    pub length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: String,
    pub classification: Status,
    pub stage: String,
    pub source: Option<SourceSpan>,
    pub message: String,
    pub opcode: Option<u8>,
    pub version: Option<String>,
    pub impact: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HsfError {
    pub diagnostic: Box<Diagnostic>,
}

impl fmt::Display for HsfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.diagnostic.code, self.diagnostic.message)
    }
}
impl std::error::Error for HsfError {}

fn error(code: &str, status: Status, message: &str) -> HsfError {
    HsfError {
        diagnostic: Box::new(Diagnostic {
            code: code.into(),
            severity: "error".into(),
            stage: if status == Status::LimitExceeded {
                "limits"
            } else {
                "hsf_header"
            }
            .into(),
            classification: status,
            source: None,
            message: message.into(),
            opcode: None,
            version: None,
            impact: None,
        }),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeResult {
    pub recognized: bool,
    pub header: Option<Header>,
    pub diagnostic: Option<Diagnostic>,
}

/// Inspect at most 64 bytes. Header recognition makes no opcode/version support claim.
pub fn probe(data: &[u8]) -> ProbeResult {
    if !data.starts_with(b";; HSF V") {
        return ProbeResult {
            recognized: false,
            header: None,
            diagnostic: None,
        };
    }
    match read_header(data) {
        Ok(header) => ProbeResult {
            recognized: true,
            header: Some(header),
            diagnostic: None,
        },
        Err(e) => ProbeResult {
            recognized: true,
            header: None,
            diagnostic: Some(*e.diagnostic),
        },
    }
}

pub fn read_header(data: &[u8]) -> Result<Header, HsfError> {
    let prefix = &data[..data.len().min(MAX_HEADER_BYTES)];
    let invalid = || {
        error(
            "hsf.invalid_header",
            Status::Invalid,
            "Expected an ASCII HSF version line terminated by LF within 64 bytes",
        )
    };
    let end = prefix
        .iter()
        .position(|&b| b == b'\n')
        .ok_or_else(invalid)?;
    let line = std::str::from_utf8(&prefix[..end]).map_err(|_| invalid())?;
    let raw = line.strip_prefix(";; HSF V").ok_or_else(invalid)?;
    let raw = raw.trim_end_matches([' ', '\r']);
    let (major, minor) = raw.split_once('.').ok_or_else(invalid)?;
    if major.is_empty()
        || major.len() > 5
        || minor.len() != 2
        || !major.bytes().all(|b| b.is_ascii_digit())
        || !minor.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid());
    }
    Ok(Header {
        version: Version {
            major: major.parse().map_err(|_| invalid())?,
            minor: minor.parse().map_err(|_| invalid())?,
            text: raw.into(),
        },
        byte_len: (end + 1) as u64,
        text: line.trim_end_matches([' ', '\r']).into(),
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub header: bool,
    pub opcodes: bool,
    pub internal_zlib: bool,
    pub scene: bool,
    pub verified_header_versions: Vec<String>,
    pub syntax_versions: Vec<String>,
    pub syntax_opcodes: Vec<u8>,
    pub scene_features: Vec<String>,
}
pub fn capabilities() -> Capabilities {
    Capabilities {
        header: true,
        opcodes: true,
        internal_zlib: true,
        scene: true,
        verified_header_versions: ["8.05", "14.50", "19.10"].map(String::from).to_vec(),
        syntax_versions: SYNTAX_VERSIONS
            .iter()
            .map(|(major, minor)| format!("{major}.{minor:02}"))
            .collect(),
        syntax_opcodes: SYNTAX_OPCODES.to_vec(),
        scene_features: [
            "uncompressed_shell",
            "triangles_convex_quads",
            "strips_fans",
            "vertex_normals",
            "modelling_matrix",
            "face_rgb",
            "face_visibility",
            "segment_references",
            "geometry_references",
            "repeat_object_subset",
            "line_polyline",
            "circle_arc_control_points",
            "text_ascii_latin1_unicode16_unicode32",
            "text_utf8_code10",
            "geometry_mask_attributes",
            "segment_user_options",
            "world_handedness",
            "named_views",
            "extended_visibility",
            "extended_rgb",
            "line_weight",
            "edge_weight",
            "pause_continuation",
            "segment_cameras",
            "texture_definition_inventory",
            "encoded_image_inventory",
            "texture_image_name_matches",
            "indexed_color_inventory",
            "conditions_inventory",
            "rectangular_mesh",
            "marker_points",
            "poly_polyline_zero_axes",
            "poly_polyline_shared_axes",
            "text_region_inventory",
            "polygon_vertices",
            "opaque_color_maps",
            "shell_optional_attribute_inventory",
            "texture_matrix_inventory",
            "texture_matrix_inheritance",
            "stored_vertex_parameters",
            "stored_parameter_widths_1_to_128",
            "quantized_vertex_parameters",
            "compressed_vertex_normals",
            "polyhedron_entity_metadata",
            "shell_refinement_parameter_inventory",
            "null_shell_lod0_geometry",
            "decoded_texture_options",
            "hidden_line_simple_shadow_inventory",
            "named_style_inventory",
            "basic_line_edge_patterns",
            "custom_dash_blank_styles",
            "parallel_dash_blank_styles",
            "line_style_fit_absolute_contrast",
            "line_style_transformed_request",
            "file_root_line_style_bindings",
            "basic_marker_symbols",
            "text_alignment_inventory",
            "text_alignment_inheritance",
            "typed_rendering_operands",
            "glyph_programs",
            "file_root_marker_glyph_bindings",
            "clip_region_inventory",
            "segment_user_indices",
            "rendering_options_inventory",
            "null_shells",
            "implicit_segment_parents",
            "standard_segment_alias_inventory",
            "explicit_import_context_inventory",
            "style_segment_reference_inventory",
            "unconditional_style_display",
            "diffuse_texture_requests",
            "local_style_image_associations",
            "marker_size",
            "line_style_definition_inventory",
            "font_definition_inventory",
            "opaque_user_data_inventory",
            "opaque_terminal_user_data_inventory",
            "mounted_root_relative_paths",
            "repeat_object_null_shell_instances",
            "best_effort_placement",
        ]
        .map(String::from)
        .to_vec(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildInfo {
    pub version: String,
    pub source_sha256: String,
    pub contract_version: u32,
    pub capabilities: Capabilities,
}
pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: VERSION.into(),
        source_sha256: SOURCE_SHA256.into(),
        contract_version: CONTRACT_VERSION,
        capabilities: capabilities(),
    }
}

/// Zero permits no consumption; it never means unlimited.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limits {
    pub max_input_bytes: u64,
    pub max_expanded_bytes: u64,
    /// Conservative cumulative allocation/output charge. Release does not refund it.
    pub max_output_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 256 << 20,
            max_expanded_bytes: 512 << 20,
            max_output_bytes: 768 << 20,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input_bytes: u64,
    pub expanded_bytes: u64,
    pub output_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct Budget {
    limits: Limits,
    usage: Usage,
}
impl Budget {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            usage: Usage::default(),
        }
    }
    pub fn usage(&self) -> &Usage {
        &self.usage
    }
    pub fn charge_input(&mut self, bytes: u64) -> Result<(), HsfError> {
        charge(
            &mut self.usage.input_bytes,
            bytes,
            self.limits.max_input_bytes,
            "budget.input",
        )
    }
    pub fn charge_expanded(&mut self, bytes: u64) -> Result<(), HsfError> {
        charge(
            &mut self.usage.expanded_bytes,
            bytes,
            self.limits.max_expanded_bytes,
            "budget.expanded",
        )
    }
    pub fn charge_output(&mut self, bytes: u64) -> Result<(), HsfError> {
        charge(
            &mut self.usage.output_bytes,
            bytes,
            self.limits.max_output_bytes,
            "budget.output",
        )
    }
}
fn charge(current: &mut u64, bytes: u64, max: u64, code: &str) -> Result<(), HsfError> {
    let next = current
        .checked_add(bytes)
        .filter(|&n| n <= max)
        .ok_or_else(|| error(code, Status::LimitExceeded, "Resource budget exceeded"))?;
    *current = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headers_preserve_integer_versions_and_boundaries() {
        for text in ["8.05", "14.50", "19.10", "20.90"] {
            let bytes = format!(";; HSF V{text} \nI\0\0");
            let h = read_header(bytes.as_bytes()).unwrap();
            assert_eq!(h.version.text, text);
            assert_eq!(h.byte_len as usize, bytes.find('\n').unwrap() + 1);
        }
        assert_eq!(read_header(b";; HSF V8.05\r\n").unwrap().version.minor, 5);
        for data in [
            b";; HSF V8.5\n".as_slice(),
            b";; HSF V8.05",
            b";; HSF V8.05junk\n",
            b";; HSF V-8.05\n",
        ] {
            assert!(read_header(data).is_err());
        }
        assert!(!probe(b"not HSF").recognized);
        assert!(probe(b";; HSF V8.05").diagnostic.is_some());
        let mut long = b";; HSF V8.05".to_vec();
        long.extend([b' '; 80]);
        long.push(b'\n');
        assert!(read_header(&long).is_err());
    }
    #[test]
    fn nested_budget_never_resets_or_wraps() {
        let mut b = Budget::new(Limits {
            max_input_bytes: 0,
            max_expanded_bytes: 100,
            max_output_bytes: u64::MAX,
        });
        b.charge_expanded(60).unwrap();
        assert!(b.charge_expanded(60).is_err());
        assert_eq!(b.usage().expanded_bytes, 60);
        assert!(b.charge_input(1).is_err());
        b.charge_output(u64::MAX).unwrap();
        assert!(b.charge_output(1).is_err());
    }
}
