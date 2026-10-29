//! A deliberately bounded generic scene profile, independent of CAD products.
use crate::*;
use std::collections::{HashMap, HashSet};
mod text_settings;
pub use text_settings::*;
mod glyph;
pub use glyph::*;
mod context;
pub use context::*;
mod line_style;
mod mesh;
pub use line_style::*;
mod patterns;
mod polyhedron;
pub use patterns::*;
mod primitive;
pub use primitive::*;
mod view;
pub use view::*;
mod image;
pub use image::*;
mod segments;
pub use segments::*;
mod styles;
pub use styles::*;
mod texture;
pub use texture::*;
mod display;
pub use display::*;

pub type Matrix = [f64; 16];
pub const IDENTITY: Matrix = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SceneOptions {
    /// Explicit absolute import location for partial inventory; external state is unknown.
    pub import_root: Option<String>,
    pub mode: ParseMode,
    pub max_meshes: u64,
    pub max_primitives: u64,
    pub max_nodes: u64,
    pub max_instances: u64,
    pub max_depth: u64,
    pub max_visits: u64,
    pub max_triangles: u64,
    pub max_views: u64,
    pub max_definitions: u64,
    /// Explicit adapter authorization for one root User Data record. Continue
    /// inventory only; all placements are withheld. None preserves fail-closed.
    pub inventory_user_data_record: Option<String>,
    /// Separate authorization for a final opaque User Data record before z/0.
    /// Requires the first inventory record and never enables placements.
    pub inventory_terminal_user_data_record: Option<String>,
    /// Absolute runtime segment path at which the producer mounted this file's
    /// root. Relative paths that climb above the file root continue through the
    /// mount's ancestors: results inside the mount map back to file paths and
    /// results outside it are external and never modeled. None keeps the file
    /// root as the only root, so root-escaping paths stay unsupported.
    pub mount_path: Option<String>,
    /// Verified (default) withholds every placement while unresolved state could
    /// change the arrangement. BestEffort traverses anyway in partial mode and
    /// labels the result an estimate; strict mode rejects it.
    pub placement: PlacementPolicy,
}
impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            mode: ParseMode::Partial,
            import_root: None,
            max_meshes: 100_000,
            max_primitives: 100_000,
            max_nodes: 100_000,
            max_instances: 1_000_000,
            max_depth: 256,
            max_visits: 2_000_000,
            max_triangles: 10_000_000,
            max_views: 10_000,
            max_definitions: 10_000,
            inventory_user_data_record: None,
            inventory_terminal_user_data_record: None,
            mount_path: None,
            placement: PlacementPolicy::Verified,
        }
    }
}
/// How unresolved global state affects instance placement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementPolicy {
    /// Placements are withheld whenever unresolved state could change them.
    #[default]
    Verified,
    /// Placements are computed from the stored tree, matrices and references and
    /// reported as estimates (`hsf.placement_estimated`). Partial mode only.
    BestEffort,
}

/// Stored all-vertex normals in local coordinates. No normalization or transform.
#[derive(Clone, Debug, Serialize)]
pub struct VertexNormals {
    pub count: u32,
    pub source: SourceSpan,
    pub values_source: SourceSpan,
    pub encoding: String,
    pub compression: Option<u8>,
    pub bits_per_sample: Option<u8>,
    pub packing: String,
    /// Implicit quantization bounds; empty for uncompressed XYZ.
    pub bounds: Vec<[f32; 2]>,
}
/// Ordered geometry-level user strings; product-specific keys remain opaque.
#[derive(Clone, Debug, Serialize)]
pub struct EntityMetadata {
    pub source: SourceSpan,
    pub fields: Vec<EntityMetadataField>,
    pub decoding_status: Status,
    pub interpretation_status: Status,
}
#[derive(Clone, Debug, Serialize)]
pub struct EntityMetadataField {
    pub kind: String,
    pub text: TextValue,
}
#[derive(Clone, Debug, Serialize)]
pub struct VertexParameters {
    pub quantization: Option<ParameterQuantization>,
    pub width: u8,
    pub count: u32,
    pub source: SourceSpan,
    pub values_source: SourceSpan,
}
#[derive(Clone, Debug, Serialize)]
pub struct ParameterQuantization {
    pub bits_per_sample: u8,
    /// u8_xyz (legacy width 3 / 8 bit) or le32_msb.
    pub packing: String,
    /// One [minimum, maximum] pair per component; no UV normalization.
    pub bounds: Vec<[f32; 2]>,
    pub bounds_source: SourceSpan,
}
#[derive(Clone, Debug, Serialize)]
pub struct ShellParameterSet {
    pub mesh_id: Option<String>,
    pub id: String,
    pub node_id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub vertices_source: SourceSpan,
    pub refinement_index: Option<u32>,
    pub lod: u8,
    /// Tag association at this opcode, not replacement or vertex correspondence.
    pub target_id: Option<String>,
    pub association_status: Status,
    pub association_reason: Option<String>,
    pub vertex_parameters: VertexParameters,
    pub interpretation_status: Status,
    #[serde(skip)]
    pub parameters: Vec<f32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Mesh {
    /// Definition owner. Refinements retain the target Null Shell's owner.
    pub owner_node_id: String,
    pub vertex_normals: Option<VertexNormals>,
    pub entity_metadata: Option<EntityMetadata>,
    pub refinement: Option<ShellRefinement>,
    pub vertex_parameters: Option<VertexParameters>,
    #[serde(skip)]
    pub parameters: Vec<f32>,
    pub id: String,
    pub source: SourceSpan,
    pub source_record_id: String,
    pub vertex_count: u64,
    pub face_value_count: u64,
    pub triangle_count: u64,
    pub normal_count: u64,
    pub topology: String,
    pub triangulation: Status,
    pub bounds: Option<[[f32; 3]; 2]>,
    #[serde(skip)]
    pub vertices: Vec<[f32; 3]>,
    #[serde(skip)]
    pub face_list: Vec<i32>,
    #[serde(skip)]
    pub triangles: Vec<[u32; 3]>,
    /// Index of the source face/strip/fan for each output triangle.
    #[serde(skip)]
    pub triangle_faces: Vec<u32>,
    #[serde(skip)]
    pub normals: Vec<[f32; 3]>,
}
/// Geometry binding only. Optional attribute and overall scene status are separate.
#[derive(Clone, Debug, Serialize)]
pub struct ShellRefinement {
    pub index: u32,
    pub lod: u8,
    pub context_node_id: String,
    pub target_id: String,
    pub target_node_id: String,
    pub application_status: Status,
    pub application_reason: Option<String>,
    pub attribute_status: Status,
}
#[derive(Clone, Debug, Serialize)]
pub struct Attribute<T> {
    pub value: T,
    pub source: SourceSpan,
}
#[derive(Clone, Debug, Serialize)]
pub struct Node {
    pub implicit: bool,
    pub world_handedness: Option<Attribute<Handedness>>,
    pub display: DisplayAttributes,
    pub id: String,
    pub name: Option<String>,
    pub name_source: Option<SourceSpan>,
    pub parent_id: Option<String>,
    pub source: Option<SourceSpan>,
    pub local_transform: Option<Attribute<Matrix>>,
    pub face_color: Option<Attribute<[u8; 3]>>,
    pub face_visible: Option<Attribute<bool>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Reference {
    pub id: String,
    pub node_id: String,
    pub target_id: Option<String>,
    pub source: SourceSpan,
    pub kind: String,
    pub transform: Option<Matrix>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Instance {
    pub display: DisplayAttributes,
    pub world_handedness: Option<Attribute<Handedness>>,
    pub id: String,
    pub mesh_id: String,
    pub node_id: String,
    pub path: Vec<String>,
    /// Row-major, row-vector convention: point * local * parent.
    pub world_transform: Matrix,
    pub determinant: f64,
    pub face_color: Option<Attribute<[u8; 3]>>,
    pub face_visible: Option<Attribute<bool>>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Scene {
    pub shell_parameter_sets: Vec<ShellParameterSet>,
    pub import_context: Option<SceneImportContext>,
    pub null_shells: Vec<NullShell>,
    pub rendering_options: Vec<RenderingOptions>,
    pub line_styles: Vec<LineStyleDefinition>,
    pub glyphs: Vec<GlyphDefinition>,
    pub fonts: Vec<FontDefinition>,
    pub views: Vec<NamedView>,
    pub cameras: Vec<SegmentCamera>,
    pub textures: Vec<TextureDefinition>,
    pub images: Vec<ImageDefinition>,
    pub segment_aliases: Vec<SegmentAlias>,
    pub style_segments: Vec<StyleSegmentReference>,
    pub style_resolutions: Vec<StyleResolution>,
    pub texture_uses: Vec<TextureUse>,
    pub opaque_user_data: Vec<OpaqueUserData>,
    pub placement_status: Status,
    pub primitives: Vec<Primitive>,
    pub primitive_instances: Vec<PrimitiveInstance>,
    pub user_options: Vec<UserOptions>,
    pub user_indices: Vec<UserIndices>,
    /// HSF coordinates have no inferred unit.
    pub units: Option<String>,
    pub meshes: Vec<Mesh>,
    pub nodes: Vec<Node>,
    pub references: Vec<Reference>,
    pub instances: Vec<Instance>,
    /// Syntax records consumed before a semantic stop (independent of unparsed bytes).
    pub processed_records: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct OpaqueUserData {
    pub source_record_id: String,
    pub source: SourceSpan,
    pub profile: String,
}
#[derive(Clone, Debug)]
enum Object {
    NullShell(usize),
    Image(usize),
    Node(usize),
    Mesh(usize, Matrix),
    Primitive(usize, Matrix),
}
#[derive(Clone)]
struct Link {
    object: Object,
    reference: Option<usize>,
}
/// An in-place Repeat Object whose source placeholder may be bound later.
struct PendingInstance {
    to: usize,
    from: usize,
    transform: Matrix,
    source: SourceSpan,
    opcode: u8,
}

fn fail(r: &Record, code: &str, status: Status, message: &str) -> HsfError {
    let mut e = error(code, status, message);
    e.diagnostic.stage = if e.diagnostic.classification == Status::LimitExceeded {
        "limits"
    } else {
        "scene"
    }
    .into();
    e.diagnostic.source = Some(r.source.clone());
    e.diagnostic.opcode = Some(r.opcode);
    e.diagnostic.impact =
        Some("Scene is incomplete; unsupported geometry or placement is not fabricated".into());
    e
}
fn limit(n: u64, max: u64, code: &str) -> Result<(), HsfError> {
    if n > max {
        Err(error(
            code,
            Status::LimitExceeded,
            "Scene resource limit exceeded",
        ))
    } else {
        Ok(())
    }
}
fn multiply(a: Matrix, b: Matrix) -> Result<Matrix, HsfError> {
    let mut c = [0.; 16];
    for i in 0..4 {
        for j in 0..4 {
            c[i * 4 + j] = (0..4).map(|k| a[i * 4 + k] * b[k * 4 + j]).sum();
        }
    }
    if c.iter().any(|v| !v.is_finite()) {
        return Err(error(
            "hsf.invalid_transform",
            Status::Invalid,
            "Transform product is not finite",
        ));
    }
    Ok(c)
}
fn determinant(m: Matrix) -> f64 {
    m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8])
        + m[2] * (m[4] * m[9] - m[5] * m[8])
}
fn matrix(bytes: &[u8]) -> Result<Matrix, HsfError> {
    let mut m = IDENTITY;
    for (i, v) in bytes.chunks_exact(4).enumerate() {
        let f = f32::from_le_bytes(v.try_into().expect("four bytes"));
        if !f.is_finite() {
            return Err(error(
                "hsf.invalid_transform",
                Status::Invalid,
                "Non-finite modelling matrix",
            ));
        }
        m[(i / 3) * 4 + i % 3] = f64::from(f);
    }
    Ok(m)
}

/// Convert the verified syntax prefix into a generic scene, using the SAME
/// cumulative budget as parsing and any enclosing container decompression.
pub fn scene(
    parsed: &ParseOutcome<HsfDocument>,
    options: &SceneOptions,
    budget: &mut Budget,
) -> Result<ParseOutcome<Scene>, HsfError> {
    if options.mode == ParseMode::Strict && parsed.status != Status::Complete {
        return Err(HsfError {
            diagnostic: Box::new(parsed.diagnostics.first().cloned().unwrap_or_else(|| {
                *error(
                    "hsf.incomplete_syntax",
                    Status::Unsupported,
                    "Syntax is incomplete",
                )
                .diagnostic
            })),
        });
    }
    if options.mode == ParseMode::Strict && options.placement == PlacementPolicy::BestEffort {
        let mut e = error(
            "hsf.placement_policy_rejected",
            Status::Unsupported,
            "Best-effort placement is a partial-mode estimate; strict mode only accepts verified placements",
        );
        e.diagnostic.stage = "scene".into();
        return Err(e);
    }
    let mut b = Builder {
        doc: &parsed.data,
        options,
        budget,
        data: Scene {
            shell_parameter_sets: vec![],
            import_context: None,
            null_shells: vec![],
            rendering_options: vec![],
            line_styles: vec![],
            glyphs: vec![],
            fonts: vec![],
            views: vec![],
            cameras: vec![],
            textures: vec![],
            images: vec![],
            segment_aliases: vec![],
            style_segments: vec![],
            style_resolutions: vec![],
            texture_uses: vec![],
            opaque_user_data: vec![],
            placement_status: Status::NotRequested,
            units: None,
            primitives: vec![],
            primitive_instances: vec![],
            user_options: vec![],
            user_indices: vec![],
            meshes: vec![],
            nodes: vec![],
            references: vec![],
            instances: vec![],
            processed_records: 0,
        },
        diagnostics: parsed.diagnostics.clone(),
        links: vec![],
        paths: HashMap::new(),
        node_paths: vec![],
        node_depths: vec![],
        tags: HashMap::new(),
        null_meshes: HashMap::new(),
        mount: None,
        pending_instances: vec![],
        include_targets: HashSet::new(),
        refinement_parameters: HashMap::new(),
        full_mesh_count: 0,
        refinement_count: 0,
        unresolved_refinement: false,
        pending_line_patterns: vec![],
        pending_glyphs: vec![],
        stack: vec![0],
        last: None,
        triangles: 0,
        primitive_parts: 0,
        import_node: 0,
        initial_nodes: 1,
        style_edges: vec![],
        styled_displays: vec![],
        visits: 0,
    };
    b.node(None, Some("/".into()), None, None)?;
    b.paths.insert("/".into(), 0);
    b.node_paths[0] = Some("/".into());
    b.initialize_context()?;
    b.initialize_mount()?;
    for r in &parsed.data.records {
        if let Err(mut e) = b.record(r) {
            if e.diagnostic.stage == "hsf_header" {
                e.diagnostic.stage = "scene".into();
            }
            e.diagnostic.version = Some(parsed.data.header.version.text.clone());
            if e.diagnostic.source.is_none() {
                e.diagnostic.source = Some(r.source.clone());
                e.diagnostic.opcode = Some(r.opcode);
            }
            if e.diagnostic.classification == Status::Unsupported
                && options.mode == ParseMode::Partial
            {
                b.budget.charge_output(512)?;
                // Attributes or scopes after this point cannot safely be applied.
                // Keep definitions for inspection, but suppress all placements.
                b.match_image_names()?;
                b.resolve_texture_uses(false)?;
                b.resolve_glyphs(false)?;
                b.resolve_line_patterns(false)?;
                b.resolve_styles(false)?;
                let stopped = e.diagnostic.source.clone();
                b.diagnostics.push(*e.diagnostic);
                if options.placement == PlacementPolicy::BestEffort {
                    b.resolve_instances()?;
                    let mut reasons = b.withholding_reasons();
                    reasons.push("stopped_at_unsupported_record");
                    b.estimate_placements(&reasons, stopped.as_ref())?;
                    return Ok(b.outcome(parsed, true));
                }
                return Ok(b.outcome(parsed, false));
            }
            return Err(e);
        }
        b.data.processed_records += 1;
    }
    b.match_image_names()?;
    b.resolve_texture_uses(parsed.unparsed.is_empty())?;
    b.resolve_glyphs(parsed.unparsed.is_empty())?;
    b.resolve_line_patterns(parsed.unparsed.is_empty())?;
    b.resolve_styles(parsed.unparsed.is_empty())?;
    b.resolve_instances()?;
    let reasons = b.withholding_reasons();
    if !reasons.is_empty() {
        // Inventoried definitions retain local bytes. Unknown application state
        // cannot be asserted as a world placement, even with a complete suffix.
        if options.placement == PlacementPolicy::BestEffort {
            b.estimate_placements(&reasons, None)?;
            return Ok(b.outcome(parsed, true));
        }
        return Ok(b.outcome(parsed, false));
    }
    b.traverse().map_err(|mut e| {
        if e.diagnostic.stage == "hsf_header" {
            e.diagnostic.stage = "scene".into();
        }
        e.diagnostic.version = Some(parsed.data.header.version.text.clone());
        e
    })?;
    Ok(b.outcome(parsed, true))
}

struct Builder<'a> {
    doc: &'a HsfDocument,
    options: &'a SceneOptions,
    budget: &'a mut Budget,
    data: Scene,
    diagnostics: Vec<Diagnostic>,
    links: Vec<Vec<Link>>,
    paths: HashMap<String, usize>,
    node_paths: Vec<Option<String>>,
    node_depths: Vec<u64>,
    tags: HashMap<u32, Object>,
    // A target remains a stable NullShell object for tags and references.
    // None records a conflicting repeated refinement and never permits rebinding.
    null_meshes: HashMap<usize, Option<(usize, Matrix)>>,
    mount: Option<Vec<String>>,
    pending_instances: Vec<PendingInstance>,
    include_targets: HashSet<usize>,
    refinement_parameters: HashMap<usize, usize>,
    full_mesh_count: usize,
    refinement_count: usize,
    unresolved_refinement: bool,
    pending_line_patterns: Vec<line_style::PendingLinePattern>,
    pending_glyphs: Vec<glyph::PendingGlyph>,
    stack: Vec<usize>,
    last: Option<Object>,
    triangles: u64,
    primitive_parts: u64,
    import_node: usize,
    initial_nodes: usize,
    style_edges: Vec<Vec<(usize, Option<usize>)>>,
    styled_displays: Vec<Option<Box<DisplayAttributes>>>,
    visits: u64,
}
impl Builder<'_> {
    fn outcome(
        mut self,
        parsed: &ParseOutcome<HsfDocument>,
        traversed: bool,
    ) -> ParseOutcome<Scene> {
        self.data.placement_status = if !traversed {
            Status::Unsupported
        } else if parsed.status == Status::Complete && self.diagnostics.is_empty() {
            Status::Complete
        } else {
            Status::Partial
        };
        ParseOutcome {
            data: self.data,
            status: if traversed && parsed.status == Status::Complete && self.diagnostics.is_empty()
            {
                Status::Complete
            } else {
                Status::Partial
            },
            diagnostics: self.diagnostics,
            unparsed: parsed.unparsed.clone(),
            usage: self.budget.usage().clone(),
        }
    }
    fn issue(&mut self, r: &Record, code: &str, message: &str) -> Result<(), HsfError> {
        let mut e = fail(r, code, Status::Unsupported, message);
        e.diagnostic.version = Some(self.doc.header.version.text.clone());
        if self.options.mode == ParseMode::Strict {
            return Err(e);
        }
        self.budget.charge_output(512)?;
        self.diagnostics.push(*e.diagnostic);
        Ok(())
    }
    fn node(
        &mut self,
        parent: Option<usize>,
        name: Option<String>,
        name_source: Option<SourceSpan>,
        source: Option<SourceSpan>,
    ) -> Result<usize, HsfError> {
        limit(
            self.data.nodes.len() as u64 + 1,
            self.options.max_nodes,
            "hsf.limit_scene_nodes",
        )?;
        let depth = parent.map_or(1, |p| self.node_depths[p] + 1);
        limit(depth, self.options.max_depth, "hsf.limit_scene_depth")?;
        self.budget.charge_output(8192)?;
        let i = self.data.nodes.len();
        self.data.nodes.push(Node {
            implicit: false,
            world_handedness: None,
            display: DisplayAttributes::default(),
            id: format!("segment:{i}"),
            name,
            name_source,
            parent_id: parent.map(|p| self.data.nodes[p].id.clone()),
            source,
            local_transform: None,
            face_color: None,
            face_visible: None,
        });
        self.links.push(vec![]);
        self.node_paths.push(None);
        self.node_depths.push(depth);
        self.style_edges.push(vec![]);
        self.styled_displays.push(None);
        if let Some(p) = parent {
            self.links[p].push(Link {
                object: Object::Node(i),
                reference: None,
            });
        }
        Ok(i)
    }
    fn name(&self, r: &Record) -> Result<&str, HsfError> {
        let bytes = self
            .doc
            .source_bytes(r.payload.as_ref().expect("framed string"))
            .expect("framed span");
        if !bytes.is_ascii() {
            return Err(fail(
                r,
                "hsf.segment_name_encoding",
                Status::Unsupported,
                "Only ASCII segment paths have verified name semantics; raw name bytes remain in syntax",
            ));
        }
        Ok(std::str::from_utf8(bytes).expect("ASCII"))
    }
    fn target_id(&self, object: &Object) -> String {
        match object {
            Object::Node(n) => self.data.nodes[*n].id.clone(),
            Object::Image(i) => self.data.images[*i].id.clone(),
            Object::NullShell(i) => self.data.null_shells[*i].id.clone(),
            Object::Mesh(m, _) => self.data.meshes[*m].id.clone(),
            Object::Primitive(i, _) => self.data.primitives[*i].id.clone(),
        }
    }
    fn reference(
        &mut self,
        n: usize,
        r: &Record,
        object: Option<Object>,
        transform: Option<Matrix>,
    ) -> Result<(), HsfError> {
        self.budget.charge_output(1024)?;
        let i = self.data.references.len();
        self.data.references.push(Reference {
            id: format!("reference:{i}"),
            node_id: self.data.nodes[n].id.clone(),
            target_id: object.as_ref().map(|o| self.target_id(o)),
            source: r.source.clone(),
            kind: r.kind.clone(),
            transform,
        });
        if let Some(object) = object {
            self.links[n].push(Link {
                object,
                reference: Some(i),
            });
        }
        Ok(())
    }
    fn terminal_inventory_allowed(&self, r: &Record) -> bool {
        if self.options.inventory_terminal_user_data_record.as_deref() != Some(r.id.as_str())
            || r.user_data.is_none()
            || r.scope.is_some()
            || self.stack.len() != 1
            || self.stack[0] != self.import_node
            || self.data.opaque_user_data.len() != 1
            || self.options.inventory_user_data_record.as_deref()
                != Some(self.data.opaque_user_data[0].source_record_id.as_str())
            || self.data.opaque_user_data[0].source.domain_id != r.source.domain_id
        {
            return false;
        }
        let Some(tail) = self
            .doc
            .records
            .len()
            .checked_sub(3)
            .and_then(|i| self.doc.records.get(i..))
        else {
            return false;
        };
        let [last, stop, end] = tail else {
            return false;
        };
        if last.id != r.id
            || stop.opcode != b'z'
            || end.opcode != 0
            || stop.scope.is_some()
            || end.scope.is_some()
            || stop.source.domain_id != r.source.domain_id
            || end.source.domain_id == r.source.domain_id
            || stop.source.length != 1
            || end.source.length != 1
            || r.source.offset.checked_add(r.source.length) != Some(stop.source.offset)
            || self
                .doc
                .domains
                .iter()
                .any(|d| d.parsed_bytes != d.byte_len)
        {
            return false;
        }
        let Some(child) = self.doc.domains.iter().find(|d| d.id == r.source.domain_id) else {
            return false;
        };
        let Some(parent) = &child.parent else {
            return false;
        };
        let Some(root) = self
            .doc
            .domains
            .iter()
            .find(|d| d.id == end.source.domain_id)
        else {
            return false;
        };
        stop.source.offset.checked_add(1) == Some(child.byte_len)
            && end.source.offset.checked_add(1) == Some(root.byte_len)
            && parent.domain_id == root.id
            && parent.offset.checked_add(parent.length) == Some(end.source.offset)
    }
    fn record(&mut self, r: &Record) -> Result<(), HsfError> {
        let n = *self.stack.last().expect("root");
        match r.opcode {
            b'i' => self.image(r, n)?,
            b'[' => {
                if self.terminal_inventory_allowed(r) {
                    self.issue(r, "hsf.terminal_user_data_inventory",
                        "Terminal User Data retained as opaque inventory; application semantics and placements remain unsupported")?;
                    self.diagnostics
                        .last_mut()
                        .expect("issued diagnostic")
                        .severity = "warning".into();
                    self.budget.charge_output(1024)?;
                    self.data.opaque_user_data.push(OpaqueUserData {
                        source_record_id: r.id.clone(),
                        source: r.source.clone(),
                        profile: r
                            .user_data
                            .as_ref()
                            .expect("checked layout")
                            .profile
                            .clone(),
                    });
                    self.last = None;
                    return Ok(());
                }
                if self.options.inventory_user_data_record.as_deref() == Some(r.id.as_str())
                    && r.user_data.is_some()
                    && r.scope.is_none()
                    && self.stack.len() == 1
                    && self.stack[0] == self.import_node
                    && self.data.nodes.len() == self.initial_nodes
                    && self.data.primitives.is_empty()
                    && self.data.meshes.is_empty()
                    && self.data.opaque_user_data.is_empty()
                {
                    self.issue(r, "hsf.user_data_inventory",
                        "Adapter authorized opaque User Data inventory; later metadata and local definitions are retained, all placements are withheld")?;
                    self.diagnostics
                        .last_mut()
                        .expect("issued diagnostic")
                        .severity = "warning".into();
                    self.budget.charge_output(1024)?;
                    self.data.opaque_user_data.push(OpaqueUserData {
                        source_record_id: r.id.clone(),
                        source: r.source.clone(),
                        profile: r
                            .user_data
                            .as_ref()
                            .expect("checked layout")
                            .profile
                            .clone(),
                    });
                    self.last = None;
                    return Ok(());
                }
                return Err(fail(
                    r,
                    "hsf.user_data_semantics",
                    Status::Unsupported,
                    "Application User Data semantics are unknown; placements are suppressed",
                ));
            }
            b'h' => {
                let value = match r.value.expect("framed handedness") {
                    0 => Handedness::Left,
                    1 => Handedness::Right,
                    _ => unreachable!("syntax validates enum"),
                };
                self.data.nodes[n].world_handedness = Some(Attribute {
                    value,
                    source: r.source.clone(),
                });
            }
            b'>' => {
                limit(
                    (self.data.views.len() + self.data.cameras.len()) as u64 + 1,
                    self.options.max_views,
                    "hsf.limit_views",
                )?;
                self.budget.charge_output(2048)?;
                let (camera, usable) = view::decode(self.doc, r)?;
                if !usable {
                    self.issue(
                        r,
                        "hsf.degenerate_camera",
                        "Camera has no usable basis; raw assignment is retained",
                    )?;
                }
                self.data.cameras.push(SegmentCamera {
                    node_id: self.data.nodes[n].id.clone(),
                    source_record_id: r.id.clone(),
                    source: r.source.clone(),
                    camera_source: r.camera.as_ref().expect("framed camera").settings.clone(),
                    camera,
                    status: if usable {
                        Status::Complete
                    } else {
                        Status::Unsupported
                    },
                });
            }
            b't' => {
                limit(
                    self.definition_count() as u64 + 1,
                    self.options.max_definitions,
                    "hsf.limit_definitions",
                )?;
                self.budget.charge_output(2048)?;
                self.issue(r,"hsf.texture_definition","Texture definition retained; image lookup and texture effects are not evaluated")?;
                let l = r.texture.as_ref().expect("framed texture");
                let options = texture::decode_options(self.doc, l, self.budget)?;
                let name = primitive::text(self.doc, &l.name, None, self.budget)?;
                let image = primitive::text(self.doc, &l.image, None, self.budget)?;
                self.data.textures.push(TextureDefinition {
                    options,
                    id: format!("texture:{}", self.data.textures.len()),
                    source_record_id: r.id.clone(),
                    source: r.source.clone(),
                    context_node_id: self.data.nodes[n].id.clone(),
                    name,
                    image,
                    image_name_matches: vec![],
                    flags: l.flags,
                    interpretation_status: Status::Unsupported,
                });
                self.last = None;
            }
            b'}' => {
                limit(
                    (self.data.views.len() + self.data.cameras.len()) as u64 + 1,
                    self.options.max_views,
                    "hsf.limit_views",
                )?;
                self.budget.charge_output(2048)?;
                let (camera, usable) = view::decode(self.doc, r)?;
                let layout = r.view.as_ref().expect("framed view");
                let name = primitive::text(self.doc, &layout.name, None, self.budget)?;
                if !usable {
                    self.issue(
                        r,
                        "hsf.degenerate_view",
                        "Named view has no usable camera basis; raw values are retained",
                    )?;
                }
                if name.status != Status::Complete {
                    self.issue(
                        r,
                        "hsf.view_name_encoding",
                        "Named-view name encoding is unknown; raw name is retained",
                    )?;
                }
                let status = if usable && name.status == Status::Complete {
                    Status::Complete
                } else {
                    Status::Unsupported
                };
                self.data.views.push(NamedView {
                    id: format!("view:{}", self.data.views.len()),
                    source_record_id: r.id.clone(),
                    source: r.source.clone(),
                    context_node_id: self.data.nodes[n].id.clone(),
                    name,
                    camera_source: layout.camera.clone(),
                    camera,
                    status,
                });
                // Views are application presets, not segment cameras, geometry
                // tag targets, active projections, or drawing-sheet definitions.
                self.last = None;
            }
            b'(' => {
                let name = self.name(r)?.to_owned();
                let path = if name.is_empty() {
                    None
                } else {
                    Some(match self.segment_path(n, &name, r)? {
                        Some(ResolvedPath::Internal(p)) => p,
                        Some(ResolvedPath::External(_)) => {
                            return Err(fail(
                                r,
                                "hsf.segment_path_external",
                                Status::Unsupported,
                                "Segment path leaves the file root mounted at the configured mount path; external segments are not modeled",
                            ));
                        }
                        None => {
                            return Err(fail(
                                r,
                                "hsf.segment_path",
                                Status::Unsupported,
                                "Segment path is outside the supported literal path profile",
                            ));
                        }
                    })
                };
                let next = if let Some(i) = path.as_ref().and_then(|p| self.paths.get(p)) {
                    *i
                } else {
                    let parent = if let Some(p) = &path {
                        // HC_Open_Segment creates all necessary containing
                        // segments; only the final node is pushed on the stack.
                        let parts = p.split('/').filter(|x| !x.is_empty());
                        let depth = parts.clone().count();
                        limit(
                            depth as u64 + 1,
                            self.options.max_depth,
                            "hsf.limit_scene_depth",
                        )?;
                        let mut parent = 0;
                        let mut prefix = String::new();
                        self.budget.charge_output(p.len() as u64)?;
                        for part in parts.take(depth.saturating_sub(1)) {
                            prefix.push('/');
                            prefix.push_str(part);
                            if let Some(existing) = self.paths.get(&prefix) {
                                parent = *existing;
                            } else {
                                self.budget.charge_output(prefix.len() as u64 * 2)?;
                                let i = self.node(
                                    Some(parent),
                                    Some(part.into()),
                                    None,
                                    Some(r.source.clone()),
                                )?;
                                self.data.nodes[i].implicit = true;
                                self.paths.insert(prefix.clone(), i);
                                self.node_paths[i] = Some(prefix.clone());
                                parent = i;
                            }
                        }
                        parent
                    } else {
                        n
                    };
                    if let Some(p) = &path {
                        self.budget.charge_output(p.len() as u64 * 2)?;
                    }
                    let i = self.node(
                        Some(parent),
                        Some(name),
                        r.payload.clone(),
                        Some(r.source.clone()),
                    )?;
                    if let Some(p) = path {
                        self.paths.insert(p.clone(), i);
                        self.node_paths[i] = Some(p);
                    }
                    i
                };
                self.stack.push(next);
                self.last = Some(Object::Node(next));
            }
            b')' => {
                self.last = self.stack.pop().map(Object::Node);
            }
            b's' => {
                let Some(Object::Node(i)) = self.tags.get(&r.value.expect("tag")) else {
                    return Err(fail(
                        r,
                        "hsf.unresolved_reopen",
                        Status::Unsupported,
                        "Reopen target is not a previously tagged segment",
                    ));
                };
                self.stack.push(*i);
                self.last = Some(Object::Node(*i));
            }
            b'q' => {
                if let Some(o) = &self.last {
                    self.budget.charge_output(256)?;
                    self.tags.insert(r.value.expect("tag"), o.clone());
                } else {
                    self.issue(
                        r,
                        "hsf.unknown_tag_target",
                        "Tag target is not a supported object",
                    )?;
                }
            }
            b'%' => {
                self.data.nodes[n].local_transform = Some(Attribute {
                    value: matrix(
                        self.doc
                            .source_bytes(r.payload.as_ref().expect("matrix"))
                            .expect("span"),
                    )?,
                    source: r.source.clone(),
                });
            }
            b'j' => self.glyph_definition(r, n)?,
            b'*' => self.text_alignment(r, n)?,
            b'J' | b'f' => self.definition(r, n)?,
            b'+' | b'=' | 0x0e => {
                let l = r
                    .marker_size
                    .as_ref()
                    .or(r.line_weight.as_ref())
                    .or(r.edge_weight.as_ref())
                    .expect("framed size");
                let units = match l.units {
                    None => "relative",
                    Some(0) => "object",
                    Some(1) => "screen",
                    Some(2) => "window",
                    Some(3) => "points",
                    Some(4) => "pixels",
                    _ => unreachable!("syntax validates units"),
                };
                let attribute = Some(Attribute {
                    value: MarkerSize {
                        value: f32::from_bits(l.value_bits).abs(),
                        units: units.into(),
                    },
                    source: r.source.clone(),
                });
                if r.opcode == b'+' {
                    self.data.nodes[n].display.marker_size = attribute;
                } else if r.opcode == b'=' {
                    self.data.nodes[n].display.line_weight = attribute;
                } else {
                    self.data.nodes[n].display.edge_weight = attribute;
                }
            }
            b'$' => {
                self.budget.charge_output(4096)?;
                self.data.nodes[n].display.texture_matrix = Some(Attribute {
                    value: matrix(
                        self.doc
                            .source_bytes(r.payload.as_ref().expect("texture matrix"))
                            .expect("span"),
                    )?,
                    source: r.source.clone(),
                });
                self.data.rendering_options.push(RenderingOptions {
                    line_style_binding: None,
                    glyph_binding: None,
                    settings: None,
                    node_id: self.data.nodes[n].id.clone(),
                    source_record_id: r.id.clone(),
                    source: r.source.clone(),
                    kind: r.kind.clone(),
                    interpretation_status: Status::Complete,
                });
            }
            b'@' | b'-' | 0x0d => self.pattern_attribute(r, n)?,
            b'H' | b'F' | b'"' | b'R' | b'O' | b'?' | 8 | 9 | 0x0c => {
                self.budget.charge_output(2048)?;
                let settings = self.render_settings(r)?;
                self.issue(r, "hsf.rendering_semantics", "Render setting operands are retained; their effects are not evaluated and placements are withheld")?;
                if let Some(color) = &r.indexed_color {
                    for bit in 0..32 {
                        if color.geometry_mask & 0x017f7f7f & (1 << bit) != 0 {
                            self.data.nodes[n].display.colors[bit] = None;
                        }
                    }
                    self.data.nodes[n].face_color = self.data.nodes[n].display.colors[0].clone();
                }
                if r.opcode == b'"' {
                    self.texture_use(r, n)?;
                    let layout = r.rendering.as_ref().expect("framed material");
                    if layout.value & 1 != 0 {
                        // An unevaluated diffuse assignment supersedes known
                        // RGB values. Do not expose a stale local face color.
                        for bit in 0..32 {
                            if layout.mask & 0x017f7f7f & (1 << bit) != 0 {
                                self.data.nodes[n].display.colors[bit] = None;
                            }
                        }
                        self.data.nodes[n].face_color =
                            self.data.nodes[n].display.colors[0].clone();
                    }
                }
                self.data.rendering_options.push(RenderingOptions {
                    line_style_binding: None,
                    glyph_binding: None,
                    settings,
                    node_id: self.data.nodes[n].id.clone(),
                    source_record_id: r.id.clone(),
                    source: r.source.clone(),
                    kind: r.kind.clone(),
                    interpretation_status: Status::Unsupported,
                });
            }
            b'V' => {
                let l = r.visibility.as_ref().expect("framed visibility");
                if l.mask & 0x0400_0000 != 0 {
                    self.budget.charge_output(2048)?;
                    self.issue(r, "hsf.visibility_geometry_bit26",
                        "Visibility bit 26 is retained as opaque state; its geometry meaning and placements are unverified")?;
                    self.data.rendering_options.push(RenderingOptions {
                        line_style_binding: None,
                        glyph_binding: None,
                        settings: None,
                        node_id: self.data.nodes[n].id.clone(),
                        source_record_id: r.id.clone(),
                        source: r.source.clone(),
                        kind: "visibility".into(),
                        interpretation_status: Status::Unsupported,
                    });
                }
                for bit in 0..32 {
                    if l.mask & 0x037f_ff7f & (1 << bit) != 0 {
                        self.data.nodes[n].display.visibility[bit] = Some(Attribute {
                            value: l.value & (1 << bit) != 0,
                            source: r.source.clone(),
                        });
                    }
                }
                self.data.nodes[n].face_visible = self.data.nodes[n].display.visibility[0].clone();
            }
            b'~' => {
                let mask = r.value.expect("geometry mask");
                let bytes = self
                    .doc
                    .source_bytes(r.payload.as_ref().expect("attribute"))
                    .expect("span");
                for bit in 0..32 {
                    if mask & 0x017f7f7f & (1 << bit) == 0 {
                        continue;
                    }
                    self.data.nodes[n].display.colors[bit] = Some(Attribute {
                        value: bytes.try_into().expect("rgb"),
                        source: r.source.clone(),
                    });
                }
                self.data.nodes[n].face_color = self.data.nodes[n].display.colors[0].clone();
            }
            b'X' | 0x10 | b'l' | b'L' | b'G' | b'C' | b'c' | b'T' | b'x' => {
                if r.primitive
                    .as_ref()
                    .is_some_and(|p| p.text_region.is_some())
                {
                    self.budget.charge_output(2048)?;
                    self.issue(r, "hsf.text_region_semantics",
                        "Text region is retained; coordinate space and glyph layout are not evaluated and placements are withheld")?;
                    self.data.rendering_options.push(RenderingOptions {
                        line_style_binding: None,
                        glyph_binding: None,
                        settings: None,
                        node_id: self.data.nodes[n].id.clone(),
                        source_record_id: r.id.clone(),
                        source: r.source.clone(),
                        kind: r.kind.clone(),
                        interpretation_status: Status::Unsupported,
                    });
                }
                let parts = r
                    .poly_polyline
                    .as_ref()
                    .map_or(1, |l| u64::from(l.primitive_count));
                charge(
                    &mut self.primitive_parts,
                    parts,
                    self.options.max_primitives,
                    "hsf.limit_primitives",
                )?;
                let mut p = primitive::decode(self.doc, r, self.budget)?;
                if p.text
                    .as_ref()
                    .is_some_and(|t| t.status != Status::Complete)
                {
                    self.issue(
                        r,
                        "hsf.text_encoding",
                        "Text encoding is not supported; raw bytes and placement are retained",
                    )?;
                }
                let i = self.data.primitives.len();
                self.budget
                    .charge_output(128 + self.data.nodes[n].id.len() as u64)?;
                p.id = format!("primitive:{i}");
                p.owner_node_id = self.data.nodes[n].id.clone();
                self.data.primitives.push(p);
                let object = Object::Primitive(i, IDENTITY);
                self.links[n].push(Link {
                    object: object.clone(),
                    reference: None,
                });
                self.last = Some(object);
            }
            b'n' => {
                self.budget.charge_output(1024)?;
                self.data.user_indices.push(UserIndices {
                    node_id: self.data.nodes[n].id.clone(),
                    source_record_id: r.id.clone(),
                    source: r.source.clone(),
                    layout: r.user_index.clone().expect("framed user index"),
                    interpretation_status: Status::Unsupported,
                });
            }
            b'U' | 0x16 => {
                let text = primitive::text(
                    self.doc,
                    r.payload.as_ref().expect("options"),
                    if r.opcode == 0x16 { Some(5) } else { None },
                    self.budget,
                )?;
                // Metadata decoding has its own status; generic scene completeness
                // does not claim configuration/component/sheet semantics.
                self.data.user_options.push(UserOptions {
                    node_id: self.data.nodes[n].id.clone(),
                    source_record_id: r.id.clone(),
                    text,
                });
            }
            b'S' | b'M' => {
                self.last = None;
                if let Some(layout) = r.shell.as_ref().filter(|l| l.null) {
                    limit(
                        (self.data.meshes.len()
                            + self.data.null_shells.len()
                            + self.data.shell_parameter_sets.len()) as u64
                            + 1,
                        self.options.max_meshes,
                        "hsf.limit_meshes",
                    )?;
                    self.budget.charge_output(1024)?;
                    let i = self.data.null_shells.len();
                    self.data.null_shells.push(NullShell {
                        mesh_id: None,
                        instance_of: None,
                        instance_transform: None,
                        id: format!("null_shell:{i}"),
                        node_id: self.data.nodes[n].id.clone(),
                        source_record_id: r.id.clone(),
                        source: r.source.clone(),
                        lod: layout.lod,
                    });
                    self.links[n].push(Link {
                        object: Object::NullShell(i),
                        reference: None,
                    });
                    self.last = Some(Object::NullShell(i));
                    return Ok(());
                }
                if r.shell
                    .as_ref()
                    .is_some_and(|layout| layout.refinement_index.is_some() || layout.lod != 0)
                {
                    self.refine_shell(r, n)?;
                    return Ok(());
                }
                limit(
                    (self.data.meshes.len()
                        + self.data.null_shells.len()
                        + self.data.shell_parameter_sets.len()) as u64
                        + 1,
                    self.options.max_meshes,
                    "hsf.limit_meshes",
                )?;
                if r.mesh.as_ref().is_some_and(|m| {
                    m.attributes
                        .iter()
                        .any(|a| ![1, 3, 17, 27, 99].contains(&a.opcode))
                }) || r.shell.as_ref().is_some_and(|s| {
                    s.attributes
                        .iter()
                        .any(|a| ![1, 3, 17, 27, 99].contains(&a.opcode))
                }) {
                    self.budget.charge_output(2048)?;
                    self.issue(r,"hsf.mesh_attributes","Geometry is retained; optional attributes are not evaluated and placements are withheld")?;
                    self.data.rendering_options.push(RenderingOptions {
                        line_style_binding: None,
                        glyph_binding: None,
                        settings: None,
                        node_id: self.data.nodes[n].id.clone(),
                        source_record_id: r.id.clone(),
                        source: r.source.clone(),
                        kind: r.kind.clone(),
                        interpretation_status: Status::Unsupported,
                    });
                }
                let (mut m, unsupported) =
                    mesh::decode(self.doc, r, self.options, &mut self.triangles, self.budget)?;
                if let Some(reason) = unsupported {
                    self.issue(r, "hsf.shell_triangulation", reason)?;
                }
                let i = self.data.meshes.len();
                self.budget
                    .charge_output(128 + self.data.nodes[n].id.len() as u64)?;
                m.id = format!("mesh:{}", self.full_mesh_count);
                m.owner_node_id = self.data.nodes[n].id.clone();
                self.full_mesh_count += 1;
                self.data.meshes.push(m);
                let object = Object::Mesh(i, IDENTITY);
                self.links[n].push(Link {
                    object: object.clone(),
                    reference: None,
                });
                self.last = Some(object);
            }
            b'r' | b'<' => {
                let mut external = false;
                let target = if r.opcode == b'r' {
                    self.tags.get(&r.value.expect("index")).cloned()
                } else if r.condition.is_none() {
                    let name = self.name(r)?.to_owned();
                    match self.segment_path(n, &name, r)? {
                        Some(ResolvedPath::Internal(p)) => {
                            self.paths.get(&p).copied().map(Object::Node)
                        }
                        Some(ResolvedPath::External(_)) => {
                            external = true;
                            None
                        }
                        None => None,
                    }
                } else {
                    None
                };
                if let Some(Object::Node(t)) = &target {
                    self.budget.charge_output(64)?;
                    self.include_targets.insert(*t);
                }
                if external {
                    self.issue(
                        r,
                        "hsf.reference_external",
                        "Include target lies outside the file root mounted at the configured mount path; its branch is omitted",
                    )?;
                } else if target.is_none() {
                    self.issue(
                        r,
                        "hsf.unresolved_reference",
                        "Reference is unresolved or conditional; its branch is omitted",
                    )?;
                }
                self.reference(n, r, target, None)?;
                self.last = None;
            }
            b'&' => {
                self.last = None;
                let bytes = self
                    .doc
                    .source_bytes(r.payload.as_ref().expect("repeat"))
                    .expect("span");
                let ints: Vec<i32> = bytes[..16]
                    .chunks_exact(4)
                    .map(|b| i32::from_le_bytes(b.try_into().expect("four")))
                    .collect();
                let options = bytes[16];
                let transform = matrix(&bytes[17..])?;
                // TK_Instance order: source index/variant, destination index/variant,
                // options. Only variant 0 and the tristrip option bit (0x01) are known.
                if ints[0] < 0 || ints[1] != 0 || ints[2] < -1 || ints[3] != 0 || options & !1 != 0
                {
                    self.issue(
                        r,
                        "hsf.repeat_variant",
                        "Only default geometry variants with known options are supported",
                    )?;
                    self.reference(n, r, None, Some(transform))?;
                    return Ok(());
                }
                let source = self.tags.get(&(ints[0] as u32)).cloned();
                if ints[2] >= 0 {
                    // The destination is an existing tagged Null Shell placeholder
                    // that becomes an in-place instance of the source placeholder's
                    // bound geometry. It stays in its own segment; no link is added.
                    let destination = self.tags.get(&(ints[2] as u32)).cloned();
                    match (source, destination) {
                        (Some(Object::NullShell(from)), Some(Object::NullShell(to)))
                            if from != to =>
                        {
                            self.budget.charge_output(1024)?;
                            let i = self.data.references.len();
                            self.data.references.push(Reference {
                                id: format!("reference:{i}"),
                                node_id: self.data.null_shells[to].node_id.clone(),
                                target_id: Some(self.data.null_shells[from].id.clone()),
                                source: r.source.clone(),
                                kind: r.kind.clone(),
                                transform: Some(transform),
                            });
                            self.data.null_shells[to].instance_of =
                                Some(self.data.null_shells[from].id.clone());
                            self.data.null_shells[to].instance_transform = Some(transform);
                            self.pending_instances.push(PendingInstance {
                                to,
                                from,
                                transform,
                                source: r.source.clone(),
                                opcode: r.opcode,
                            });
                            self.last = Some(Object::NullShell(to));
                        }
                        _ => {
                            self.issue(
                                r,
                                "hsf.repeat_target",
                                "Repeat destination must be a previously tagged Null Shell distinct from a tagged Null Shell source",
                            )?;
                            self.reference(n, r, None, Some(transform))?;
                        }
                    }
                    return Ok(());
                }
                let target = match source {
                    Some(Object::Mesh(i, from)) => {
                        Some(Object::Mesh(i, multiply(from, transform)?))
                    }
                    Some(Object::Primitive(i, from)) => {
                        Some(Object::Primitive(i, multiply(from, transform)?))
                    }
                    _ => None,
                };
                if target.is_none() {
                    self.issue(
                        r,
                        "hsf.repeat_target",
                        "Repeat target is not previously tagged supported geometry",
                    )?;
                }
                self.last = target.clone();
                self.reference(n, r, target, Some(transform))?;
            }
            b'{' => self.style_segment(r, n)?,
            _ => {}
        }
        Ok(())
    }
    fn initialize_mount(&mut self) -> Result<(), HsfError> {
        let Some(requested) = &self.options.mount_path else {
            return Ok(());
        };
        let invalid = || {
            let mut e = error(
                "hsf.invalid_mount_path",
                Status::Invalid,
                "Mount path must be an absolute unquoted ASCII literal path of at most 4096 bytes; empty or navigation components, aliases and wildcards are forbidden",
            );
            e.diagnostic.stage = "scene".into();
            e
        };
        if requested.len() > 4096
            || !requested.starts_with('/')
            || requested
                .bytes()
                .any(|b| !(b.is_ascii_alphanumeric() || b" /#+-$_ .".contains(&b)))
        {
            return Err(invalid());
        }
        self.budget
            .charge_output(1024 + requested.len() as u64 * 4)?;
        let components: Vec<String> = if requested == "/" {
            vec![]
        } else {
            requested[1..]
                .split('/')
                .map(|s| {
                    s.split(' ')
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_ascii_lowercase()
                })
                .collect()
        };
        if components
            .iter()
            .any(|s| matches!(s.as_str(), "" | "." | ".." | "..." | "...."))
        {
            return Err(invalid());
        }
        self.mount = Some(components);
        Ok(())
    }
    fn instance_diagnostic(
        &mut self,
        p: &PendingInstance,
        code: &str,
        message: &str,
    ) -> Result<(), HsfError> {
        let mut e = error(code, Status::Unsupported, message);
        e.diagnostic.stage = "scene".into();
        e.diagnostic.source = Some(p.source.clone());
        e.diagnostic.opcode = Some(p.opcode);
        e.diagnostic.version = Some(self.doc.header.version.text.clone());
        e.diagnostic.impact =
            Some("Scene is incomplete; unsupported geometry or placement is not fabricated".into());
        if self.options.mode == ParseMode::Strict {
            return Err(e);
        }
        self.budget.charge_output(512)?;
        self.diagnostics.push(*e.diagnostic);
        Ok(())
    }
    /// Bind in-place Repeat Object instances once their source placeholders have
    /// geometry. Sources may be refined after the instance record, so iterate.
    fn resolve_instances(&mut self) -> Result<(), HsfError> {
        let mut pending = std::mem::take(&mut self.pending_instances);
        let mut progress = true;
        while progress && !pending.is_empty() {
            progress = false;
            let mut remaining = Vec::with_capacity(pending.len());
            for p in pending {
                match self.null_meshes.get(&p.from).copied() {
                    Some(Some((mesh, local))) => {
                        progress = true;
                        let composed = multiply(local, p.transform)?;
                        if self.null_meshes.contains_key(&p.to) {
                            // A placeholder with its own update or a second instance
                            // assignment is ambiguous; no winner is selected.
                            self.unresolved_refinement = true;
                            self.null_meshes.insert(p.to, None);
                            self.data.null_shells[p.to].mesh_id = None;
                            self.instance_diagnostic(
                                &p,
                                "hsf.repeat_target_bound",
                                "Repeat destination already has geometry; its binding is revoked",
                            )?;
                        } else {
                            self.budget.charge_output(512)?;
                            self.null_meshes.insert(p.to, Some((mesh, composed)));
                            self.data.null_shells[p.to].mesh_id =
                                Some(self.data.meshes[mesh].id.clone());
                        }
                    }
                    Some(None) => {
                        progress = true;
                        self.unresolved_refinement = true;
                        self.instance_diagnostic(&p, "hsf.repeat_source_unbound", "Repeat source Null Shell has no usable geometry; the instance has none")?;
                    }
                    None => remaining.push(p),
                }
            }
            pending = remaining;
        }
        for p in pending {
            self.unresolved_refinement = true;
            self.instance_diagnostic(
                &p,
                "hsf.repeat_source_unbound",
                "Repeat source Null Shell has no usable geometry; the instance has none",
            )?;
        }
        Ok(())
    }
    /// State that keeps verified placements from being asserted.
    fn withholding_reasons(&self) -> Vec<&'static str> {
        let mut reasons = vec![];
        if !self.data.opaque_user_data.is_empty() {
            reasons.push("opaque_user_data");
        }
        if self.unresolved_refinement {
            reasons.push("unresolved_refinement");
        }
        if self
            .data
            .glyphs
            .iter()
            .any(|g| g.interpretation_status != Status::Complete)
        {
            reasons.push("glyph_definitions");
        }
        if self
            .data
            .line_styles
            .iter()
            .any(|d| d.interpretation_status != Status::Complete)
        {
            reasons.push("line_style_definitions");
        }
        if self
            .data
            .rendering_options
            .iter()
            .any(|x| x.interpretation_status != Status::Complete)
        {
            reasons.push("rendering_options");
        }
        if !self.data.textures.is_empty() {
            reasons.push("textures");
        }
        if !self.data.images.is_empty() {
            reasons.push("images");
        }
        if self.data.import_context.is_some() {
            reasons.push("import_context");
        }
        if !self.data.segment_aliases.is_empty() {
            reasons.push("segment_aliases");
        }
        if self
            .data
            .style_resolutions
            .iter()
            .any(|s| s.status != Status::Complete)
        {
            reasons.push("style_resolutions");
        }
        reasons
    }
    /// Segments inside the standard Include Library are definitions: when they
    /// are include targets they render only through their includes, never at
    /// their own tree position. The mount path supplies the runtime location of
    /// the file root, so a mounted file's library segments are recognized too.
    fn library_definition(&self, n: usize) -> bool {
        if !self.include_targets.contains(&n) {
            return false;
        }
        let Some(path) = &self.node_paths[n] else {
            return false;
        };
        let under = |p: &str| p == "/include library" || p.starts_with("/include library/");
        if under(path) {
            return true;
        }
        match &self.mount {
            Some(mount) => {
                let suffix = if path == "/" { "" } else { path.as_str() };
                under(&format!("/{}{}", mount.join("/"), suffix))
            }
            None => false,
        }
    }
    /// Best-effort placement: record the estimate and traverse anyway.
    fn estimate_placements(
        &mut self,
        reasons: &[&str],
        stopped: Option<&SourceSpan>,
    ) -> Result<(), HsfError> {
        self.budget.charge_output(2048)?;
        let mut d = error(
            "hsf.placement_estimated",
            Status::Partial,
            &format!(
                "Placements are estimated from the stored segment tree, modelling matrices and references while unresolved state is ignored: {}",
                reasons.join(", ")
            ),
        );
        d.diagnostic.severity = "warning".into();
        d.diagnostic.stage = "scene".into();
        d.diagnostic.source = stopped.cloned();
        d.diagnostic.version = Some(self.doc.header.version.text.clone());
        d.diagnostic.impact = Some(
            "Instances are best-effort estimates; verify the arrangement against the producing application".into(),
        );
        self.diagnostics.push(*d.diagnostic);
        self.traverse()
    }
    fn traverse(&mut self) -> Result<(), HsfError> {
        #[derive(Clone)]
        struct Frame {
            world_handedness: Option<Attribute<Handedness>>,
            node: usize,
            next: usize,
            matrix: Matrix,
            color: Option<Attribute<[u8; 3]>>,
            visible: Option<Attribute<bool>>,
            path: Vec<String>,
            display: DisplayAttributes,
        }
        let root = &self.data.nodes[0];
        limit(1, self.options.max_depth, "hsf.limit_scene_depth")?;
        self.budget.charge_output(8192)?;
        let mut frames = vec![Frame {
            world_handedness: root.world_handedness.clone(),
            node: 0,
            next: 0,
            matrix: root.local_transform.as_ref().map_or(IDENTITY, |a| a.value),
            color: self.styled_displays[0]
                .as_deref()
                .unwrap_or(&root.display)
                .colors[0]
                .clone(),
            visible: self.styled_displays[0]
                .as_deref()
                .unwrap_or(&root.display)
                .visibility[0]
                .clone(),
            path: vec![root.id.clone()],
            display: self.styled_displays[0]
                .as_deref()
                .unwrap_or(&root.display)
                .clone(),
        }];
        while let Some(f) = frames.last_mut() {
            if f.next == self.links[f.node].len() {
                frames.pop();
                continue;
            }
            self.visits += 1;
            limit(
                self.visits,
                self.options.max_visits,
                "hsf.limit_scene_visits",
            )?;
            let link = self.links[f.node][f.next].clone();
            f.next += 1;
            self.budget
                .charge_output(16384 + f.path.iter().map(|s| s.len() as u64 + 64).sum::<u64>())?;
            let mut path = f.path.clone();
            if let Some(i) = link.reference {
                path.push(self.data.references[i].id.clone());
            }
            let object = match link.object {
                Object::NullShell(i) => match self.null_meshes.get(&i).copied().flatten() {
                    Some((m, local)) => Object::Mesh(m, local),
                    None => Object::NullShell(i),
                },
                other => other,
            };
            match object {
                Object::NullShell(_) | Object::Image(_) => {}
                Object::Mesh(i, local) => {
                    limit(
                        (self.data.instances.len() + self.data.primitive_instances.len()) as u64
                            + 1,
                        self.options.max_instances,
                        "hsf.limit_instances",
                    )?;
                    let world = multiply(local, f.matrix)?;
                    let det = determinant(world);
                    if !det.is_finite() {
                        return Err(error(
                            "hsf.invalid_transform",
                            Status::Invalid,
                            "Transform determinant is not finite",
                        ));
                    }
                    self.data.instances.push(Instance {
                        display: f.display.clone(),
                        world_handedness: f.world_handedness.clone(),
                        id: format!("instance:{}", self.data.instances.len()),
                        mesh_id: self.data.meshes[i].id.clone(),
                        node_id: self.data.nodes[f.node].id.clone(),
                        path,
                        world_transform: world,
                        determinant: det,
                        face_color: f.color.clone(),
                        face_visible: f.visible.clone(),
                    });
                }
                Object::Primitive(i, local) => {
                    limit(
                        (self.data.instances.len() + self.data.primitive_instances.len()) as u64
                            + 1,
                        self.options.max_instances,
                        "hsf.limit_instances",
                    )?;
                    let world = multiply(local, f.matrix)?;
                    let det = determinant(world);
                    if !det.is_finite() {
                        return Err(error(
                            "hsf.invalid_transform",
                            Status::Invalid,
                            "Transform determinant is not finite",
                        ));
                    }
                    self.data.primitive_instances.push(PrimitiveInstance {
                        world_handedness: f.world_handedness.clone(),
                        id: format!("primitive_instance:{}", self.data.primitive_instances.len()),
                        primitive_id: self.data.primitives[i].id.clone(),
                        node_id: self.data.nodes[f.node].id.clone(),
                        path,
                        world_transform: world,
                        determinant: det,
                        display: f.display.clone(),
                    });
                }
                Object::Node(n) => {
                    if link.reference.is_none() && self.library_definition(n) {
                        continue;
                    }
                    let node = &self.data.nodes[n];
                    let display = self.styled_displays[n]
                        .as_deref()
                        .unwrap_or(&node.display)
                        .inherit(&f.display);
                    let matrix = multiply(
                        node.local_transform.as_ref().map_or(IDENTITY, |a| a.value),
                        f.matrix,
                    )?;
                    let color = display.colors[0].clone();
                    let visible = display.visibility[0].clone();
                    let world_handedness = node
                        .world_handedness
                        .clone()
                        .or_else(|| f.world_handedness.clone());
                    if frames.iter().any(|f| f.node == n) {
                        let reference =
                            &self.data.references[link.reference.expect("tree edges cannot cycle")];
                        let mut e = error(
                            "hsf.reference_cycle",
                            Status::Unsupported,
                            "Cyclic include branch omitted",
                        );
                        e.diagnostic.stage = "scene".into();
                        e.diagnostic.source = Some(reference.source.clone());
                        e.diagnostic.version = Some(self.doc.header.version.text.clone());
                        if self.options.mode == ParseMode::Strict {
                            return Err(e);
                        }
                        self.diagnostics.push(*e.diagnostic);
                        continue;
                    }
                    limit(
                        frames.len() as u64 + 1,
                        self.options.max_depth,
                        "hsf.limit_scene_depth",
                    )?;
                    path.push(node.id.clone());
                    frames.push(Frame {
                        world_handedness,
                        node: n,
                        next: 0,
                        matrix,
                        color,
                        visible,
                        path,
                        display,
                    });
                }
            }
        }
        Ok(())
    }
}
