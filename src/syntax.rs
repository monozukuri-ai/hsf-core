//! Framing is derived from the public HOOPS HSF opcode reference. Payload bytes
//! remain owned in source domains; record spans never imply a text encoding.
use crate::*;
use flate2::{Decompress, FlushDecompress, Status as ZStatus};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::sync::Arc;
mod continuation;
mod image;
pub use image::*;
mod progress;
pub use progress::*;
mod rendering;
pub use continuation::*;
mod settings;
pub use rendering::*;
pub use settings::*;

pub const SYNTAX_OPCODES: &[u8] = &[
    0, b';', b'I', b'B', b'b', b'(', b')', b's', b'<', b'{', b'r', b'q', b'U', b'v', b'Z', b'z',
    b'S', b'%', b'~', b'V', b'&', b'l', b'L', b'C', b'c', b'T', b'x', 0x16, b'h', b'}', b'J', b'f',
    b'+', b'H', b'F', b'"', b'=', 0x0e, b'@', b'*', b'R', b'O', b'n', b'-', 0x0d, 1, b't', b'>',
    b'?', 8, 9, b'M', b'X', b'$', 0x10, b'G', 0x0c, b'i', b'j', b'0', b'm', b'!', b'P', b'E', b'e',
];
pub const SYNTAX_VERSIONS: &[(u16, u16)] = &[
    (8, 5),
    (14, 50),
    (17, 85),
    (18, 0),
    (19, 0),
    (19, 10),
    (20, 90),
    (21, 0),
    (21, 15),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseMode {
    Strict,
    #[default]
    Partial,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ParseOptions {
    pub image_profile: ImageProfile,
    pub max_image_pixels: u64,
    pub mode: ParseMode,
    pub max_records: u64,
    pub max_segment_depth: u64,
    pub max_domains: u64,
    pub max_string_bytes: u64,
    pub max_compression_ratio: u64,
    pub max_vertices: u64,
    pub max_face_values: u64,
}
impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            image_profile: ImageProfile::Disabled,
            max_image_pixels: 16_777_216,
            mode: ParseMode::Partial,
            max_records: 1_000_000,
            max_segment_depth: 256,
            max_domains: 128,
            max_string_bytes: 16 << 20,
            max_compression_ratio: 1000,
            max_vertices: 10_000_000,
            max_face_values: 50_000_000,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceDomain {
    pub id: Arc<str>,
    pub byte_len: u64,
    pub sha256: String,
    pub parent: Option<SourceSpan>,
    pub encoding: String,
    /// Contiguous framing prefix, including compressed payloads in their parent
    /// domain. Semantic coverage of decoded bytes is reported independently.
    pub parsed_bytes: u64,
    #[serde(skip)]
    bytes: Arc<[u8]>,
}
impl SourceDomain {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Shared with the scene objects that cite this record.
    pub id: Arc<str>,
    pub opcode: u8,
    pub kind: Cow<'static, str>,
    pub source: SourceSpan,
    /// Syntactic scope only (the enclosing open-segment record's id). Reopened
    /// segment identities are not resolved.
    pub scope: Option<Arc<str>>,
    /// Operand bytes without length prefixes. Text encodings remain unspecified.
    pub payload: Option<SourceSpan>,
    pub condition: Option<SourceSpan>,
    /// File flags, reference index, bounds type, user value, or implicit tag index.
    pub value: Option<u32>,
    /// The opcode's own framing; read it with the accessors (`shell()` and so on).
    pub layout: Option<RecordLayout>,
}

/// Opcode-specific framing. A record carries at most one: large layouts are
/// boxed and the few small ones inline, so a record without one stays small
/// (a large file holds millions of records).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordLayout {
    Shell(Box<ShellLayout>),
    PolyPolyline(Box<PolyPolylineLayout>),
    Primitive(Box<PrimitiveLayout>),
    UserData(Box<UserDataLayout>),
    View(Box<ViewLayout>),
    Visibility(VisibilityLayout),
    LineStyle(Box<LineStyleLayout>),
    Glyph(Box<GlyphLayout>),
    Font(Box<FontLayout>),
    MarkerSize(MarkerSizeLayout),
    LineWeight(MarkerSizeLayout),
    EdgeWeight(MarkerSizeLayout),
    NamedStyle(Box<NamedStyleLayout>),
    ClipRegion(Box<ClipRegionLayout>),
    UserIndex(Box<UserIndexLayout>),
    UserIndexData(Box<UserIndexDataLayout>),
    Rendering(Box<RenderingLayout>),
    Camera(Box<CameraLayout>),
    Texture(Box<TextureLayout>),
    Image(Box<ImageLayout>),
    IndexedColor(IndexedColorLayout),
    ColorMap(Box<ColorMapLayout>),
    Mesh(Box<GridMeshLayout>),
}

macro_rules! layout_accessors {
    ($($name:ident: $variant:ident => $ty:ty;)*) => {
        impl Record {
            $(
                pub fn $name(&self) -> Option<&$ty> {
                    match &self.layout {
                        Some(RecordLayout::$variant(l)) => Some(std::borrow::Borrow::borrow(l)),
                        _ => None,
                    }
                }
            )*
        }
    };
}
layout_accessors! {
    shell: Shell => ShellLayout;
    poly_polyline: PolyPolyline => PolyPolylineLayout;
    primitive: Primitive => PrimitiveLayout;
    user_data: UserData => UserDataLayout;
    view: View => ViewLayout;
    visibility: Visibility => VisibilityLayout;
    line_style: LineStyle => LineStyleLayout;
    glyph: Glyph => GlyphLayout;
    font: Font => FontLayout;
    marker_size: MarkerSize => MarkerSizeLayout;
    line_weight: LineWeight => MarkerSizeLayout;
    edge_weight: EdgeWeight => MarkerSizeLayout;
    named_style: NamedStyle => NamedStyleLayout;
    clip_region: ClipRegion => ClipRegionLayout;
    user_index: UserIndex => UserIndexLayout;
    user_index_data: UserIndexData => UserIndexDataLayout;
    rendering: Rendering => RenderingLayout;
    camera: Camera => CameraLayout;
    texture: Texture => TextureLayout;
    image: Image => ImageLayout;
    indexed_color: IndexedColor => IndexedColorLayout;
    color_map: ColorMap => ColorMapLayout;
    mesh: Mesh => GridMeshLayout;
}

/// Raw masks include extension flags; only mask-selected geometry bits apply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibilityLayout {
    pub mask: u32,
    pub value: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineStyleLayout {
    pub name: SourceSpan,
    pub definition: SourceSpan,
}
pub type GlyphLayout = LineStyleLayout;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontLayout {
    pub format: u8,
    pub name: SourceSpan,
    /// Glyph mapping code, not the encoding of the name or lookup string.
    pub encoding: u8,
    pub lookup: SourceSpan,
    pub data: SourceSpan,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkerSizeLayout {
    /// Exact IEEE-754 wire bits, including negative zero.
    pub value_bits: u32,
    pub units: Option<u8>,
}

/// RGB8 palette entries in wire order. Index lookup and inheritance are not
/// evaluated by the scene builder; the original bytes stay in the source domain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorMapLayout {
    pub format: u8,
    pub entry_count: u32,
    pub colors: SourceSpan,
}

/// Named-camera framing, not a segment camera or CAD drawing view assignment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewLayout {
    pub projection: u8,
    /// Eleven required f32s followed by the flag-selected optional f32s.
    pub camera: SourceSpan,
    pub name: SourceSpan,
}

/// Application-specific, experimental framing. No product semantics are implied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserDataLayout {
    pub profile: String,
    pub declared_length: u32,
    pub computed_payload_length: u32,
    pub fields: Vec<UserDataField>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserDataField {
    pub label: String,
    pub source: SourceSpan,
}
pub struct UserDataContext<'a> {
    pub header: &'a Header,
    pub domain_id: &'a str,
    pub offset: u64,
    pub compressed: bool,
    /// Full decoded domain length, not the length of the bounded callback bytes.
    pub domain_byte_len: u64,
    pub segment_depth: usize,
    /// Owned parent bytes after this compressed domain. For context checks only;
    /// framing still cannot seek or resynchronize in the parent domain.
    pub parent_suffix: Option<&'a [u8]>,
    pub preceding_records: &'a [Record],
    pub max_string_bytes: u64,
}
/// A trusted adapter may propose framing; the backend independently checks ranges,
/// the declared word and stop marker. Strict mode never accepts this experimental
/// extension. Input begins with '[' and is bounded to 4096 bytes by default.
/// An adapter may request more, up to the hard 1 MiB cap. No resync hook.
pub trait UserDataProfile: Sync {
    fn max_framing_bytes(&self) -> usize {
        4096
    }
    fn frame(
        &self,
        context: UserDataContext<'_>,
        bytes: &[u8],
    ) -> Result<Option<UserDataLayout>, HsfError>;
}

/// Source coordinates remain three dimensional, including drawing primitives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimitiveLayout {
    pub point_count: u32,
    pub points: SourceSpan,
    pub center: Option<SourceSpan>,
    /// None for ordinary ASCII text; numeric values belong to TKO_Text_Encodings.
    pub encoding: Option<u8>,
    pub text: Option<SourceSpan>,
    pub text_region: Option<TextRegionLayout>,
    /// Elliptical arcs: the parametric start and end (two f32, 0–1).
    #[serde(default)]
    pub limits: Option<SourceSpan>,
}

/// Raw region controls and coordinates. Space, alignment and glyph layout are
/// not evaluated; reference descriptions disagree about the space bit polarity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRegionLayout {
    pub options: u8,
    pub point_count: u32,
    pub points: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellLayout {
    pub null: bool,
    pub flags: u8,
    pub extended_flags: u16,
    pub refinement_index: Option<u32>,
    pub lod: u8,
    pub vertex_count: u32,
    /// f32 XYZ triples, or the packed codes when `point_quantization` is set.
    pub vertices: SourceSpan,
    pub point_quantization: Option<PointQuantization>,
    pub face_value_count: u32,
    pub face_bits: u8,
    pub faces: SourceSpan,
    pub normals: Option<SourceSpan>,
    pub attributes: Vec<ShellAttributeLayout>,
    /// Set for connectivity-compressed Shells (flag 0x40), whose stream holds the
    /// points and triangles; `vertices` and `faces` are then empty.
    pub connectivity: Option<ConnectivityLayout>,
}

/// A connectivity-compressed Shell's stream. Its header gives the point count;
/// the scene decodes the stream (see the supported profile there).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectivityLayout {
    /// Shell compression scheme; only 5 (edgebreaker) is framed.
    pub scheme: u8,
    /// Header, opcodes, auxiliary table, then quantized points and normals.
    pub stream: SourceSpan,
    /// Stream layout; only 2 is framed.
    pub stream_scheme: u8,
    pub points_scheme: u8,
    pub normals_scheme: u8,
    /// One triangle per opcode, before hole-plug triangles are removed.
    pub opcode_count: u32,
}

/// Byte ranges of a connectivity stream's sections, from its 24-byte header
/// (layout 2): the opcodes follow the header, and the auxiliary table and the
/// points each start at the next four-byte boundary.
pub(crate) struct ConnectivitySections {
    pub ops: std::ops::Range<usize>,
    pub table: std::ops::Range<usize>,
    pub points: std::ops::Range<usize>,
    pub points_count: usize,
}

pub(crate) fn connectivity_sections(stream: &[u8]) -> Result<ConnectivitySections, HsfError> {
    let invalid = |message| error("hsf.invalid_shell_connectivity", Status::Invalid, message);
    if stream.len() < 24 {
        return Err(invalid("Connectivity stream is shorter than its header"));
    }
    if stream[0] != 2 || stream[1] != 0 {
        return Err(error(
            "hsf.shell_connectivity_scheme",
            Status::Unsupported,
            "Only connectivity stream layout 2 with table layout 0 is supported",
        ));
    }
    if stream[2] == 0 {
        return Err(error(
            "hsf.shell_connectivity_points",
            Status::Unsupported,
            "Connectivity streams without quantized points are not implemented",
        ));
    }
    let word = |at: usize| {
        usize::try_from(i32::from_le_bytes(
            stream[at..at + 4].try_into().expect("four bytes"),
        ))
        .map_err(|_| invalid("Negative connectivity section length"))
    };
    let (ops, table, points, points_count) = (word(4)?, word(8)?, word(12)?, word(16)?);
    let align = |n: usize| n.checked_add(3).map(|n| n & !3);
    let ops = 24..24 + ops;
    let table_start = align(ops.end).ok_or_else(|| invalid("Connectivity sections overflow"))?;
    let table = table_start..table_start.saturating_add(table);
    let points_start = align(table.end).ok_or_else(|| invalid("Connectivity sections overflow"))?;
    let points = points_start..points_start.saturating_add(points);
    if points.end > stream.len() || ops.end > stream.len() {
        return Err(invalid("Connectivity sections extend beyond the stream"));
    }
    Ok(ConnectivitySections {
        ops,
        table,
        points,
        points_count,
    })
}

/// Quantized Shell points: each coordinate is a `bits_per_sample` code spanning
/// the box in `bounds` (min XYZ then max XYZ, six f32), packed like quantized
/// parameters. The scene decodes them to the mesh's f32 vertices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointQuantization {
    pub scheme: u8,
    pub bits_per_sample: u8,
    pub bounds: SourceSpan,
}

#[derive(Clone, Debug, Serialize)]
pub struct HsfDocument {
    pub header: Header,
    pub records: Vec<Record>,
    pub domains: Vec<SourceDomain>,
}
impl HsfDocument {
    pub fn source_bytes(&self, source: &SourceSpan) -> Option<&[u8]> {
        let domain = self.domains.iter().find(|d| d.id == source.domain_id)?;
        let end = source.offset.checked_add(source.length)?;
        domain
            .bytes
            .get(usize::try_from(source.offset).ok()?..usize::try_from(end).ok()?)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ParseOutcome<T> {
    pub data: T,
    pub status: Status,
    pub diagnostics: Vec<Diagnostic>,
    pub unparsed: Vec<SourceSpan>,
    pub usage: Usage,
}

/// Standalone parse: charges input and its retained copy to the supplied budget.
pub fn parse(
    data: &[u8],
    options: &ParseOptions,
    budget: &mut Budget,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    budget.charge_input(data.len() as u64)?;
    budget.charge_output(data.len() as u64)?;
    parse_precharged(Arc::from(data), options, budget)
}

/// Adapter entry point. The caller MUST have charged input ownership and any
/// outer decompression to this same budget. Does not reset or re-charge them.
pub fn parse_precharged(
    data: Arc<[u8]>,
    options: &ParseOptions,
    budget: &mut Budget,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse_precharged_with_user_data(data, options, budget, None)
}

/// As `parse_precharged`, with an explicitly selected experimental application
/// profile. Generic parsing remains unchanged when `profile` is None.
pub fn parse_precharged_with_user_data(
    data: Arc<[u8]>,
    options: &ParseOptions,
    budget: &mut Budget,
    profile: Option<&dyn UserDataProfile>,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    let header = read_header(&data).map_err(|mut e| {
        e.diagnostic.source = Some(span(0, 0, data.len().min(MAX_HEADER_BYTES)));
        e
    })?;
    let start = header.byte_len as usize;
    let mut p = Parser {
        document: HsfDocument {
            header,
            records: vec![],
            domains: vec![],
        },
        options,
        budget,
        diagnostics: vec![],
        stack: vec![],
        tag: 0,
        vertices: 0,
        image_pixels: 0,
        face_values: 0,
        user_data_profile: profile,
    };
    p.add_domain(data, None)?;
    p.document.domains[0].parsed_bytes = start as u64;
    if !SYNTAX_VERSIONS.contains(&(
        p.document.header.version.major,
        p.document.header.version.minor,
    )) {
        p.unsupported(
            0,
            0,
            None,
            "hsf.unsupported_version",
            "Version has no verified syntax profile",
        )?;
    } else {
        p.domain(0, start, false)?;
    }
    let unparsed = p
        .document
        .domains
        .iter()
        .filter(|d| d.parsed_bytes < d.byte_len)
        .map(|d| SourceSpan {
            domain_id: d.id.clone(),
            offset: d.parsed_bytes,
            length: d.byte_len - d.parsed_bytes,
        })
        .collect();
    Ok(ParseOutcome {
        status: if p.diagnostics.is_empty() {
            Status::Complete
        } else {
            Status::Partial
        },
        diagnostics: p.diagnostics,
        unparsed,
        usage: p.budget.usage().clone(),
        data: p.document,
    })
}

fn span(domain: usize, start: usize, length: usize) -> SourceSpan {
    SourceSpan {
        domain_id: domain_name(domain),
        offset: start as u64,
        length: length as u64,
    }
}

/// `hsf:{domain}`, allocated once per domain index and thread: a large file
/// holds millions of spans, and each used to own its copy of the name.
fn domain_name(domain: usize) -> Arc<str> {
    thread_local! {
        static NAMES: std::cell::RefCell<Vec<Arc<str>>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    NAMES.with(|names| {
        let mut names = names.borrow_mut();
        while names.len() <= domain {
            let next = names.len();
            names.push(Arc::from(format!("hsf:{next}")));
        }
        names[domain].clone()
    })
}

struct Parser<'a> {
    document: HsfDocument,
    options: &'a ParseOptions,
    budget: &'a mut Budget,
    diagnostics: Vec<Diagnostic>,
    stack: Vec<Arc<str>>,
    tag: u32,
    vertices: u64,
    image_pixels: u64,
    face_values: u64,
    user_data_profile: Option<&'a dyn UserDataProfile>,
}

impl Parser<'_> {
    fn fail(
        &self,
        domain: usize,
        at: usize,
        opcode: Option<u8>,
        code: &str,
        status: Status,
        message: &str,
    ) -> HsfError {
        let mut e = error(code, status, message);
        e.diagnostic.stage = if e.diagnostic.classification == Status::LimitExceeded {
            "limits"
        } else {
            "hsf_syntax"
        }
        .into();
        e.diagnostic.source = Some(span(domain, at, usize::from(opcode.is_some())));
        e.diagnostic.opcode = opcode;
        e.diagnostic.version = Some(self.document.header.version.text.clone());
        e.diagnostic.impact = Some("Parsing stops; no scene has been generated".into());
        e
    }
    fn unsupported(
        &mut self,
        d: usize,
        at: usize,
        op: Option<u8>,
        code: &str,
        message: &str,
    ) -> Result<(), HsfError> {
        let e = self.fail(d, at, op, code, Status::Unsupported, message);
        if self.options.mode == ParseMode::Strict {
            return Err(e);
        }
        self.diagnostics.push(*e.diagnostic);
        Ok(())
    }
    fn add_domain(
        &mut self,
        bytes: Arc<[u8]>,
        parent: Option<SourceSpan>,
    ) -> Result<usize, HsfError> {
        let id = self.document.domains.len();
        let context = |mut e: HsfError| {
            e.diagnostic.source = Some(parent.clone().unwrap_or_else(|| span(0, 0, 0)));
            e.diagnostic.version = Some(self.document.header.version.text.clone());
            e.diagnostic.impact = Some("Source domain could not be retained; parsing stops".into());
            e
        };
        if id as u64 >= self.options.max_domains {
            return Err(context(error(
                "hsf.limit_domains",
                Status::LimitExceeded,
                "Source domain limit exceeded",
            )));
        }
        self.budget.charge_output(512).map_err(context)?;
        self.document.domains.push(SourceDomain {
            id: domain_name(id),
            byte_len: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            encoding: if parent.is_some() { "zlib" } else { "identity" }.into(),
            parent,
            parsed_bytes: 0,
            bytes,
        });
        Ok(id)
    }
    fn inflate(&mut self, bytes: &[u8]) -> Result<(Vec<u8>, usize), HsfError> {
        let mut z = Decompress::new(true);
        let mut out = Vec::new();
        loop {
            let mut chunk = [0; 8192];
            let before_in = z.total_in();
            let before_out = z.total_out();
            let state = z
                .decompress(
                    &bytes[before_in as usize..],
                    &mut chunk,
                    FlushDecompress::None,
                )
                .map_err(|_| {
                    error(
                        "hsf.invalid_zlib",
                        Status::Invalid,
                        "Invalid zlib stream or checksum",
                    )
                })?;
            let count = z.total_out() - before_out;
            // Until StreamEnd the complete remaining input is only an upper
            // bound on compressed size. At StreamEnd use the exact consumed
            // block, so trailing outer bytes cannot dilute the ratio.
            let compressed_bound = if state == ZStatus::StreamEnd {
                z.total_in()
            } else {
                bytes.len() as u64
            };
            if z.total_out() > compressed_bound.saturating_mul(self.options.max_compression_ratio) {
                return Err(error(
                    "hsf.limit_compression_ratio",
                    Status::LimitExceeded,
                    "HSF zlib compression ratio exceeds limit",
                ));
            }
            self.budget.charge_expanded(count)?;
            self.budget.charge_output(count)?;
            out.extend_from_slice(&chunk[..count as usize]);
            if state == ZStatus::StreamEnd {
                return Ok((out, z.total_in() as usize));
            }
            if before_in == z.total_in() && count == 0 {
                return Err(error(
                    "hsf.truncated_zlib",
                    Status::Invalid,
                    "Incomplete zlib stream",
                ));
            }
        }
    }
    /// true: this stream reached its required terminator; false: partial stop.
    fn domain(&mut self, d: usize, start: usize, compressed: bool) -> Result<bool, HsfError> {
        let bytes = self.document.domains[d].bytes.clone();
        let mut c = Cursor {
            data: &bytes,
            pos: start,
        };
        loop {
            let at = c.pos;
            let Some(&op) = bytes.get(at) else {
                return Err(self.fail(
                    d,
                    at,
                    None,
                    "hsf.missing_terminator",
                    Status::Invalid,
                    "Required stream terminator is missing",
                ));
            };
            if self.document.records.len() as u64 >= self.options.max_records {
                return Err(self.fail(
                    d,
                    at,
                    Some(op),
                    "hsf.limit_records",
                    Status::LimitExceeded,
                    "Record limit exceeded",
                ));
            }
            c.pos += 1;
            let mut record = Record {
                id: Arc::from(format!("record:{}", self.document.records.len())),
                opcode: op,
                kind: Cow::Borrowed(""),
                source: span(d, at, 0),
                scope: self.stack.last().cloned(),
                payload: None,
                condition: None,
                value: None,
                layout: None,
            };
            // Cursor routines do no allocation; limits apply before retaining records.
            let decoded = self.operand(op, d, &mut c, &mut record, compressed);
            if let Err(mut e) = decoded {
                if e.diagnostic.classification == Status::Unsupported {
                    self.unsupported(d, at, Some(op), &e.diagnostic.code, &e.diagnostic.message)?;
                    return Ok(false);
                }
                e.diagnostic.source = Some(span(d, at, 1));
                e.diagnostic.opcode = Some(op);
                e.diagnostic.version = Some(self.document.header.version.text.clone());
                e.diagnostic.stage = if e.diagnostic.classification == Status::LimitExceeded {
                    "limits"
                } else {
                    "hsf_syntax"
                }
                .into();
                e.diagnostic.impact = Some("Parsing stops; no scene has been generated".into());
                return Err(e);
            }
            // The retained record: the struct, its id/kind/scope strings and the
            // span's domain id (layouts are boxed and charged where framed).
            self.budget
                .charge_output(std::mem::size_of::<Record>() as u64 + 256)
                .map_err(|e| {
                    self.fail(
                        d,
                        at,
                        Some(op),
                        &e.diagnostic.code,
                        Status::LimitExceeded,
                        &e.diagnostic.message,
                    )
                })?;
            record.source.length = (c.pos - at) as u64;
            if op == b'(' || op == b's' {
                if self.stack.len() as u64 >= self.options.max_segment_depth {
                    return Err(self.fail(
                        d,
                        at,
                        Some(op),
                        "hsf.limit_segment_depth",
                        Status::LimitExceeded,
                        "Segment depth limit exceeded",
                    ));
                }
                self.stack.push(record.id.clone());
            } else if op == b')' && self.stack.pop().is_none() {
                return Err(self.fail(
                    d,
                    at,
                    Some(op),
                    "hsf.unbalanced_segment",
                    Status::Invalid,
                    "Close segment without an open segment",
                ));
            }
            self.document.records.push(record);
            self.document.domains[d].parsed_bytes = c.pos as u64;
            if op == b'Z' {
                if self.document.domains.len() as u64 >= self.options.max_domains {
                    return Err(self.fail(
                        d,
                        at,
                        Some(op),
                        "hsf.limit_domains",
                        Status::LimitExceeded,
                        "Source domain limit exceeded",
                    ));
                }
                let (decoded, count) = self.inflate(&bytes[c.pos..]).map_err(|e| {
                    self.fail(
                        d,
                        at,
                        Some(op),
                        &e.diagnostic.code,
                        e.diagnostic.classification.clone(),
                        &e.diagnostic.message,
                    )
                })?;
                let parent = span(d, c.pos, count);
                c.pos += count;
                let child = self.add_domain(Arc::from(decoded), Some(parent))?;
                self.document.domains[d].parsed_bytes = c.pos as u64;
                if !self.domain(child, 0, true)? {
                    return Ok(false);
                }
            } else if op == 0 || op == b'z' {
                if c.pos != bytes.len() {
                    return Err(self.fail(
                        d,
                        c.pos,
                        bytes.get(c.pos).copied(),
                        "hsf.trailing_bytes",
                        Status::Invalid,
                        "Bytes follow the stream terminator",
                    ));
                }
                if op == 0 && !self.stack.is_empty() {
                    return Err(self.fail(
                        d,
                        at,
                        Some(op),
                        "hsf.unbalanced_segment",
                        Status::Invalid,
                        "File ends with open segments",
                    ));
                }
                return Ok(true);
            }
        }
    }

    fn operand(
        &mut self,
        op: u8,
        d: usize,
        c: &mut Cursor<'_>,
        r: &mut Record,
        compressed: bool,
    ) -> Result<(), HsfError> {
        r.kind = match op {
            1 => "pause",
            b'i' => { r.layout = Some(RecordLayout::Image(Box::new(self.image(d,c)?))); "experimental_image" }
            b't' => { r.layout = Some(RecordLayout::Texture(Box::new(self.texture(d,c)?))); "texture" }
            b'M' => { r.layout = Some(RecordLayout::Mesh(Box::new(self.grid_mesh(d,c)?))); "mesh" }
            b'?' => {
                let v = &self.document.header.version;
                if (v.major,v.minor) < (11,50) { return Err(error("hsf.conditions_version",Status::Unsupported,"Conditions requires HSF 11.50")); }
                let length = match c.u8()? { 254 => u64::from(c.u16()?), 255 => u64::from(c.nonnegative()?), n => u64::from(n) };
                r.payload = Some(self.payload(d,c,length)?);
                "conditions"
            }
            8 | 9 => {
                let geometry_mask = self.color_mask(c)?;
                let index = if op == 8 { u16::from(c.u8()?) } else { c.u16()? };
                r.layout = Some(RecordLayout::IndexedColor(IndexedColorLayout { geometry_mask, index }));
                "color_by_index"
            }
            0x0c => {
                let format = c.u8()?;
                if format != 0 {
                    return Err(error("hsf.color_map_format", Status::Unsupported,
                        "Only RGB8 color map framing is supported"));
                }
                let entry_count = c.nonnegative()?;
                if entry_count > 65_536 {
                    return Err(error("hsf.limit_color_map", Status::LimitExceeded,
                        "Color map exceeds 65536 entries"));
                }
                let colors = self.payload(d, c, u64::from(entry_count) * 3)?;
                r.layout = Some(RecordLayout::ColorMap(Box::new(ColorMapLayout { format, entry_count, colors })));
                "color_map"
            }
            b'h' => {
                let value = c.u8()?;
                if value > 1 {
                    return Err(error("hsf.unsupported_handedness", Status::Unsupported,
                        "Only left (0) and right (1) world handedness are supported"));
                }
                r.value = Some(u32::from(value));
                "handedness"
            }
            b'}' | b'>' => {
                self.precision_byte(c, if op == b'}' { "hsf.unsupported_view_projection" } else { "hsf.unsupported_camera_projection" })?;
                let projection = c.u8()?;
                if projection & !0x1f != 0 {
                    return Err(error(if op == b'}' { "hsf.unsupported_view_projection" } else { "hsf.unsupported_camera_projection" }, Status::Unsupported,
                        "Unknown camera projection bits; no later opcode search"));
                }
                if projection & 0x10 != 0 && (self.document.header.version.major, self.document.header.version.minor) <= (10, 0) {
                    // The reference writer omits near_limit for <=10.00 while
                    // its reader consumes by bit alone. No unambiguous old profile.
                    return Err(error(if op == b'}' { "hsf.unsupported_view_near_limit_version" } else { "hsf.unsupported_camera_near_limit_version" }, Status::Unsupported,
                        "Near-limit framing is unverified for this older HSF version"));
                }
                let count = 11 + (projection & 0x1c).count_ones();
                let camera = c.take(d, count as usize * 4)?;
                if op == b'}' {
                    let n = c.u8()?;
                    let name = self.payload(d, c, u64::from(n))?;
                    r.payload = Some(name.clone());
                    r.layout = Some(RecordLayout::View(Box::new(ViewLayout { projection, camera, name })));
                    "view"
                } else {
                    r.layout = Some(RecordLayout::Camera(Box::new(CameraLayout { projection, settings: camera })));
                    "camera"
                }
            }
            0x10 => {
                self.precision_byte(c, "hsf.unsupported_primitive_precision")?;
                r.layout = Some(RecordLayout::PolyPolyline(Box::new(self.poly_polyline(d,c)?)));
                "poly_polyline"
            }
            b'$' => {
                let at=c.pos;
                r.payload=Some(c.take(d,48)?);
                progress::finite(&c.data[at..c.pos],"hsf.nonfinite_texture_matrix")?;
                "texture_matrix"
            }
            b'X' | b'l' | b'L' | b'G' | b'C' | b'c' | b'T' | b'x' => {
                self.precision_byte(c, "hsf.unsupported_primitive_precision")?;
                r.layout = Some(RecordLayout::Primitive(Box::new(self.primitive(op, d, c)?)));
                match op {
                    b'X' => "marker",
                    b'l' => "line",
                    b'L' => "polyline",
                    b'G' => "polygon",
                    b'C' => "circle",
                    b'c' => "circular_arc",
                    b'T' => "text",
                    _ => "text_with_encoding",
                }
            }
            b'E' | b'e' => {
                r.layout = Some(RecordLayout::Primitive(Box::new(self.ellipse(op, d, c)?)));
                if op == b'E' { "ellipse" } else { "elliptical_arc" }
            }
            b'S' => {
                self.precision_byte(c, "hsf.unsupported_shell_precision")?;
                r.layout = Some(RecordLayout::Shell(Box::new(self.shell(d, c)?)));
                "shell"
            }
            b'%' | b'&' => {
                if op == b'%' {
                    self.precision_byte(c, "hsf.unsupported_matrix_precision")?;
                }
                r.payload = Some(c.take(d, if op == b'%' { 48 } else { 65 })?);
                if op == b'%' {
                    "modelling_matrix"
                } else {
                    "repeat_object"
                }
            }
            b'0' => {
                // TKE_Priority: one i32 drawing priority for the open segment. It
                // orders rendering only and never changes geometry or placement.
                r.payload = Some(c.take(d, 4)?);
                "priority"
            }
            b'!' => {
                // TKE_Selectability: a geometry mask (a second byte when bit 7 is
                // set), then the down, up, move-down, move-up and invisible masks,
                // two bytes each when extended and one otherwise. It governs
                // picking only, so the scene ignores it.
                let mut mask = u32::from(c.u8()?);
                if mask & 0x80 != 0 {
                    mask |= u32::from(c.u8()?) << 8;
                }
                r.value = Some(mask);
                r.payload = Some(c.take(d, if mask & 0x80 != 0 { 10 } else { 5 })?);
                "selectability"
            }
            b'~' => {
                r.value = Some(self.color_mask(c)?);
                r.payload = Some(c.take(d, 3)?);
                "color_rgb"
            }
            b'H' | b'F' | b'"' => {
                r.layout = Some(RecordLayout::Rendering(Box::new(self.rendering(op, d, c)?)));
                match op { b'H' => "heuristics", b'F' => "text_font", _ => "color" }
            }
            b'R' => { r.layout = Some(RecordLayout::Rendering(Box::new(self.render_options(d, c)?))); "rendering_options" }
            b'@' | b'-' | 0x0d => {
                r.layout = Some(RecordLayout::NamedStyle(Box::new(self.named_style(op, d, c)?)));
                match op { b'@' => "marker_symbol", b'-' => "line_pattern", _ => "edge_pattern" }
            }
            b'*' => { r.value = Some(u32::from(c.u8()?)); "text_alignment" }
            b'P' => {
                // TKE_Face_Pattern: one enumerated pattern byte. A named form
                // (255 and a name, as line patterns write) is unverified here.
                let pattern = c.u8()?;
                if pattern == 255 {
                    return Err(error("hsf.face_pattern", Status::Unsupported,
                        "Named face patterns are not framed"));
                }
                r.value = Some(u32::from(pattern));
                "face_pattern"
            }
            b'O' => { r.layout = Some(RecordLayout::ClipRegion(Box::new(self.clip_region(d, c)?))); "clip_region" }
            b'n' => { r.layout = Some(RecordLayout::UserIndex(Box::new(self.user_index(d, c)?))); "user_index" }
            b'm' => {
                r.layout = Some(RecordLayout::UserIndexData(Box::new(self.user_index_data(d, c)?)));
                "user_index_data"
            }
            b'V' => {
                let mut mask = u32::from(c.u8()?);
                let value_at = c.pos;
                let mut value = u32::from(c.u8()?);
                if mask & 0x80 != 0 {
                    mask |= u32::from(c.u16()?) << 8;
                    value |= u32::from(c.u16()?) << 8;
                }
                if mask & 0x0080_0000 != 0 {
                    let v = &self.document.header.version;
                    if (v.major, v.minor) < (13, 5) {
                        return Err(error("hsf.visibility_extension_version", Status::Unsupported,
                            "Secondary visibility extension requires HSF 13.05 or later"));
                    }
                    mask |= u32::from(c.u8()?) << 24;
                    value |= u32::from(c.u8()?) << 24;
                }
                // Observed in the 2026 DXF writer, in SOLIDWORKS 2025–2026
                // eDrawings at HSF 20.90 and in the Creo 10 and 11 publishers'
                // 21.00 and 21.15. The fixed-width second extension frames this
                // bit, but its geometry meaning is not established. Scene
                // evaluation retains it as opaque state.
                let v = &self.document.header.version;
                let opaque = if matches!(
                    (v.major, v.minor),
                    (19, 10) | (20, 90) | (21, 0) | (21, 15)
                ) {
                    0x0400_0000
                } else {
                    0
                };
                if mask & !(0x037f_ff7f | 0x0080_0080 | opaque) != 0 {
                    return Err(error("hsf.visibility_mask", Status::Unsupported,
                        "Visibility mask contains unknown geometry bits"));
                }
                // Preserve the legacy low value-byte span. Extended values are
                // interleaved with mask bytes and exposed by the typed layout.
                r.payload = Some(span(d, value_at, 1));
                r.value = Some(mask);
                r.layout = Some(RecordLayout::Visibility(VisibilityLayout { mask, value }));
                "visibility"
            }
            b'J' => {
                let v = &self.document.header.version;
                if (v.major, v.minor) < (11, 60) {
                    return Err(error("hsf.line_style_version", Status::Unsupported,
                        "Line style definitions require HSF 11.60 or later"));
                }
                let length = u64::from(c.u8()?);
                let name = self.payload(d, c, length)?;
                let length = u64::from(c.u16()?);
                let definition = self.payload(d, c, length)?;
                r.layout = Some(RecordLayout::LineStyle(Box::new(LineStyleLayout { name, definition })));
                "line_style"
            }
            b'j' => {
                let v = &self.document.header.version;
                if (v.major, v.minor) < (12, 0) {
                    return Err(error("hsf.glyph_version", Status::Unsupported,
                        "Glyph definitions are outside the pre-12.00 syntax profile"));
                }
                let length = u64::from(c.u8()?);
                let name = self.payload(d, c, length)?;
                let length = u64::from(c.u16()?);
                let definition = self.payload(d, c, length)?;
                r.layout = Some(RecordLayout::Glyph(Box::new(GlyphLayout { name, definition })));
                "glyph"
            }
            b'f' => {
                let format = c.u8()?;
                if format != 0 {
                    return Err(error("hsf.font_format", Status::Unsupported,
                        "Only HOOPS stroked font framing is supported"));
                }
                let length = u64::from(c.u8()?);
                let name = self.payload(d, c, length)?;
                let encoding = c.u8()?;
                let length = u64::from(c.u8()?);
                let lookup = self.payload(d, c, length)?;
                // A signed int length, despite the Byte in the HTML table.
                let length = u64::from(c.nonnegative()?);
                let data = self.payload(d, c, length)?;
                r.layout = Some(RecordLayout::Font(Box::new(FontLayout { format, name, encoding, lookup, data })));
                "font"
            }
            b'+' | b'=' | 0x0e => {
                let value_bits = c.u32()?;
                let value = f32::from_bits(value_bits);
                if !value.is_finite() {
                    return Err(error(if op == b'+' { "hsf.nonfinite_marker_size" } else if op == b'=' { "hsf.nonfinite_line_weight" } else { "hsf.nonfinite_edge_weight" }, Status::Invalid,
                        "Size operand must be finite"));
                }
                let units = if value < 0.0 { Some(c.u8()?) } else { None };
                if units.is_some_and(|u| u > 4) {
                    return Err(error(if op == b'+' { "hsf.marker_size_units" } else if op == b'=' { "hsf.line_weight_units" } else { "hsf.edge_weight_units" }, Status::Unsupported,
                        "Unknown size units"));
                }
                let layout = Some(MarkerSizeLayout { value_bits, units });
                if op == b'+' { r.layout = layout.map(RecordLayout::MarkerSize); "marker_size" }
                else if op == b'=' { r.layout = layout.map(RecordLayout::LineWeight); "line_weight" }
                else { r.layout = layout.map(RecordLayout::EdgeWeight); "edge_weight" }
            }
            0 if !compressed => "termination",
            b'z' if compressed => "stop_compression",
            0 | b'z' => {
                return Err(error(
                    "hsf.misplaced_terminator",
                    Status::Invalid,
                    "Terminator is in the wrong source domain",
                ));
            }
            b'Z' if !compressed => "start_compression",
            b'Z' => {
                return Err(error(
                    "hsf.nested_compression",
                    Status::Unsupported,
                    "Nested compression is not supported",
                ));
            }
            b';' => {
                let length = c.data[c.pos..]
                    .iter()
                    .take(
                        usize::try_from(self.options.max_string_bytes.saturating_add(1))
                            .unwrap_or(usize::MAX),
                    )
                    .position(|b| *b == b'\n')
                    .map(|n| n + 1);
                let length = match length {
                    Some(n) => n,
                    None if (c.data.len() - c.pos) as u64 > self.options.max_string_bytes => {
                        return Err(error(
                            "hsf.limit_string",
                            Status::LimitExceeded,
                            "Comment exceeds string limit",
                        ));
                    }
                    None => {
                        return Err(error(
                            "hsf.truncated_operand",
                            Status::Invalid,
                            "Comment is missing LF",
                        ));
                    }
                };
                r.payload = Some(self.payload(d, c, length as u64)?);
                "comment"
            }
            b'I' => {
                r.value = Some(c.u32()?);
                "file_info"
            }
            b'B' | b'b' => {
                self.precision_byte(c, "hsf.unsupported_bounds")?;
                let kind = c.u8()?;
                let length = match kind {
                    0 => 24,
                    1 => 16,
                    _ => {
                        return Err(error(
                            "hsf.unsupported_bounds",
                            Status::Unsupported,
                            "Unknown bounding representation",
                        ));
                    }
                };
                r.value = Some(kind as u32);
                r.payload = Some(c.take(d, length)?);
                if op == b'B' {
                    "bounding_info"
                } else {
                    "bounding"
                }
            }
            b'(' => {
                let n = c.u8()?;
                r.payload = Some(self.payload(d, c, n as u64)?);
                "open_segment"
            }
            b')' => "close_segment",
            b's' => {
                r.value = Some(c.u32()?);
                "reopen_segment"
            }
            b'r' => {
                r.value = Some(c.u32()?);
                "reference"
            }
            b'q' => {
                r.value = Some(self.tag);
                self.tag = self.tag.checked_add(1).ok_or_else(|| {
                    error(
                        "hsf.tag_overflow",
                        Status::LimitExceeded,
                        "Implicit tag index overflow",
                    )
                })?;
                "tag_implicit"
            }
            b'<' | b'{' => {
                let version = (
                    self.document.header.version.major,
                    self.document.header.version.minor,
                );
                if version >= (20, 90) {
                    // 20.90 always writes a leading zero, then the path; no
                    // condition follows (`< 00 1d /include library/…` then `)`).
                    // A nonzero first byte is unobserved and stops.
                    if c.u8()? != 0 {
                        return Err(error(
                            "hsf.unsupported_conditional_segment",
                            Status::Unsupported,
                            "Only unconditional segment references are verified for HSF 20.90",
                        ));
                    }
                    let n = c.u8()?;
                    r.payload = Some(self.payload(d, c, n as u64)?);
                } else {
                    let mut n = c.u8()?;
                    let conditional = n == 0 && version >= (11, 50);
                    if conditional {
                        n = c.u8()?;
                    }
                    r.payload = Some(self.payload(d, c, n as u64)?);
                    if conditional {
                        let n = c.u8()?;
                        r.condition = Some(self.payload(d, c, n as u64)?);
                    }
                }
                if op == b'<' {
                    "include_segment"
                } else {
                    "style_segment"
                }
            }
            b'U' | 0x16 => {
                let n = c.u16()?;
                let n = if n == u16::MAX { c.u32()? } else { n as u32 };
                let width = if op == 0x16 { 2 } else { 1 };
                r.payload = Some(self.payload(d, c, u64::from(n) * width)?);
                if op == 0x16 {
                    "unicode_options"
                } else {
                    "user_options"
                }
            }
            b'v' => {
                r.value = Some(c.u32()?);
                "user_value"
            }
            // Observed eDrawings private data does not match the public length /
            // stop-marker framing. Do not scan for ']' or guess an adjustment.
            b'[' => {
                if let Some(profile) = self.user_data_profile {
                    // Bound metadata allocation before invoking an adapter. This
                    // includes a maximum of 64 labels/spans and their projection.
                    self.budget.charge_output(16_384)?;
                    let at = c.pos - 1;
                    let max_bytes = profile.max_framing_bytes().min(1 << 20);
                    let bytes = &c.data[at..c.data.len().min(at.saturating_add(max_bytes))];
                    let parent_suffix = self.document.domains[d].parent.as_ref().and_then(|s| {
                        let parent = self.document.domains.iter().find(|p| p.id == s.domain_id)?;
                        let end = usize::try_from(s.offset.checked_add(s.length)?).ok()?;
                        parent.bytes.get(end..)
                    });
                    let context = UserDataContext {
                        header: &self.document.header,
                        domain_id: &r.source.domain_id,
                        offset: at as u64,
                        compressed,
                        domain_byte_len: c.data.len() as u64,
                        segment_depth: self.stack.len(),
                        parent_suffix,
                        preceding_records: &self.document.records,
                        max_string_bytes: self.options.max_string_bytes,
                    };
                    if let Some(layout) = profile.frame(context, bytes)? {
                        let end = 5usize.checked_add(layout.computed_payload_length as usize);
                        let valid = end.is_some_and(|end| {
                            bytes.get(end) == Some(&b']')
                                && bytes.get(1..5) == Some(layout.declared_length.to_le_bytes().as_slice())
                                && layout.profile.len() <= 128
                                && layout.fields.len() <= 64
                                && layout.fields.iter().all(|f| {
                                    f.label.len() <= 64
                                        && f.source.domain_id == r.source.domain_id
                                        && f.source.offset >= at as u64
                                        && f.source.offset.checked_add(f.source.length)
                                            .is_some_and(|n| n <= (at + end + 1) as u64)
                                })
                        });
                        if !valid {
                            return Err(error("hsf.invalid_user_data_profile", Status::Invalid,
                                "Application profile returned an invalid boundary or field range"));
                        }
                        if self.options.mode == ParseMode::Strict {
                            return Err(error("hsf.experimental_user_data", Status::Unsupported,
                                "Experimental application User Data framing is forbidden in strict mode"));
                        }
                        let mut diagnostic = *self.fail(d, at, Some(op),
                            "hsf.experimental_user_data", Status::Unsupported,
                            "User Data boundary uses an empirical application profile; native semantics are unverified").diagnostic;
                        diagnostic.severity = "warning".into();
                        diagnostic.impact = Some("Syntax continues with an experimental boundary; result remains partial".into());
                        self.diagnostics.push(diagnostic);
                        c.pos = at + end.expect("validated end") + 1;
                        r.payload = Some(span(d, at + 5, layout.computed_payload_length as usize));
                        r.layout = Some(RecordLayout::UserData(Box::new(layout)));
                        r.kind = "experimental_user_data".into();
                        return Ok(());
                    }
                }
                return Err(error(
                    "hsf.unsupported_user_data",
                    Status::Unsupported,
                    "User-data framing is not verified for this profile",
                ));
            }
            _ => {
                return Err(error(
                    "hsf.unknown_opcode",
                    Status::Unsupported,
                    "Opcode framing is not implemented; remaining bytes are not scanned",
                ));
            }
        }
        .into();
        Ok(())
    }
    fn primitive(
        &mut self,
        op: u8,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<PrimitiveLayout, HsfError> {
        let point_count = match op {
            b'l' => 2,
            b'L' | b'G' => c.nonnegative()?,
            b'C' | b'c' => 3,
            _ => 1,
        };
        if op == b'G' && point_count < 3 {
            return Err(error(
                "hsf.polygon_point_count",
                Status::Invalid,
                "Polygon requires at least three stored points",
            ));
        }
        charge(
            &mut self.vertices,
            u64::from(point_count),
            self.options.max_vertices,
            "hsf.limit_vertices",
        )?;
        let points = c.array(d, point_count, 12)?;
        if op == b'G' {
            progress::finite(
                &c.data[points.offset as usize..c.pos],
                "hsf.nonfinite_polygon",
            )?;
        }
        let version = (
            self.document.header.version.major,
            self.document.header.version.minor,
        );
        let mut layout = PrimitiveLayout {
            point_count,
            points,
            center: None,
            encoding: None,
            text: None,
            text_region: None,
            limits: None,
        };
        if matches!(op, b'C' | b'c') && version >= (12, 15) {
            let flags = c.u8()?;
            if flags & !1 != 0 {
                return Err(error(
                    "hsf.circle_flags",
                    Status::Unsupported,
                    "Unknown circle/arc flags; framing stops",
                ));
            }
            if flags & 1 != 0 {
                charge(
                    &mut self.vertices,
                    1,
                    self.options.max_vertices,
                    "hsf.limit_vertices",
                )?;
                layout.center = Some(c.take(d, 12)?);
            }
        }
        if matches!(op, b'T' | b'x') {
            if op == b'x' {
                layout.encoding = Some(c.u8()?);
            }
            let length = match c.u8()? {
                254 => u64::from(c.u16()?),
                255 => u64::from(c.u32()?),
                n => u64::from(n),
            };
            layout.text = Some(self.payload(d, c, length)?);
            if version >= (10, 2) {
                let options = c.u8()?;
                if options & !1 != 0 {
                    return Err(error(
                        "hsf.text_options",
                        Status::Unsupported,
                        "Character attributes and unknown text options are not implemented",
                    ));
                }
                if options & 1 != 0 {
                    let options = c.u8()?;
                    if options & !0x1f != 0 {
                        return Err(error(
                            "hsf.text_region_options",
                            Status::Unsupported,
                            "Unknown text region options",
                        ));
                    }
                    let point_count = u32::from(c.u8()?);
                    if point_count > 4 {
                        return Err(error(
                            "hsf.text_region_count",
                            Status::Unsupported,
                            "Text regions with more than four points are outside the supported profile",
                        ));
                    }
                    charge(
                        &mut self.vertices,
                        u64::from(point_count),
                        self.options.max_vertices,
                        "hsf.limit_vertices",
                    )?;
                    let at = c.pos;
                    let points = c.array(d, point_count, 12)?;
                    progress::finite(&c.data[at..c.pos], "hsf.nonfinite_text_region")?;
                    layout.text_region = Some(TextRegionLayout {
                        options,
                        point_count,
                        points,
                    });
                }
            }
        }
        Ok(layout)
    }
    /// Ellipse (`E`) and elliptical arc (`e`), per the public opcode reference:
    /// the center, a point on the major axis and one on the minor axis, then
    /// for arcs the parametric start and end (0–1). Whether HSF 20.90 puts its
    /// precision byte here is unobserved, so 20.90 and later stop.
    fn ellipse(
        &mut self,
        op: u8,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<PrimitiveLayout, HsfError> {
        if (
            self.document.header.version.major,
            self.document.header.version.minor,
        ) >= (20, 90)
        {
            return Err(error(
                "hsf.unsupported_primitive_precision",
                Status::Unsupported,
                "Ellipse framing is unverified for HSF 20.90 and later",
            ));
        }
        charge(
            &mut self.vertices,
            3,
            self.options.max_vertices,
            "hsf.limit_vertices",
        )?;
        let at = c.pos;
        let points = c.array(d, 3, 12)?;
        let limits = if op == b'e' {
            Some(c.take(d, 8)?)
        } else {
            None
        };
        progress::finite(&c.data[at..c.pos], "hsf.nonfinite_ellipse")?;
        Ok(PrimitiveLayout {
            point_count: 3,
            points,
            center: None,
            encoding: None,
            text: None,
            text_region: None,
            limits,
        })
    }
    fn shell(&mut self, d: usize, c: &mut Cursor<'_>) -> Result<ShellLayout, HsfError> {
        let flags = c.u8()?;
        let extended_flags = if flags & 128 != 0 { c.u16()? } else { 0 };
        // NULL takes precedence over the other storage forms. No refinement
        // index, coordinates, faces, or optional attributes follow its LOD.
        if extended_flags & 2 != 0 {
            if extended_flags & !15 != 0 {
                return Err(error(
                    "hsf.shell_flags",
                    Status::Unsupported,
                    "Unknown null shell flags",
                ));
            }
            let lod = c.u8()?;
            let empty = span(d, c.pos, 0);
            return Ok(ShellLayout {
                null: true,
                flags,
                extended_flags,
                lod,
                refinement_index: None,
                vertex_count: 0,
                vertices: empty.clone(),
                point_quantization: None,
                face_value_count: 0,
                face_bits: 0,
                faces: empty,
                normals: None,
                attributes: vec![],
                connectivity: None,
            });
        }

        if flags & 32 != 0 || extended_flags & !4 != 0 {
            return Err(error(
                "hsf.shell_flags",
                Status::Unsupported,
                "Shell flags are outside the uncompressed profile",
            ));
        }
        let refinement_index = if flags & 16 == 0 {
            Some(c.u32()?)
        } else {
            None
        };
        let lod = c.u8()?;
        if flags & 64 != 0 {
            return self.connected_shell(d, c, flags, extended_flags, refinement_index, lod);
        }
        let (vertex_count, vertices, point_quantization) = if flags & 1 != 0 {
            self.quantized_points(d, c)?
        } else {
            let vertex_count = c.nonnegative()?;
            charge(
                &mut self.vertices,
                vertex_count as u64,
                self.options.max_vertices,
                "hsf.limit_vertices",
            )?;
            (vertex_count, c.array(d, vertex_count, 12)?, None)
        };
        if c.u8()? != 1 {
            return Err(error(
                "hsf.shell_face_encoding",
                Status::Unsupported,
                "Only CS_TRIVIAL face lists are supported",
            ));
        }
        // Length counts encoded bytes, including the width byte.
        let length = c.nonnegative()?;
        if length == 0 {
            return Err(error(
                "hsf.invalid_face_encoding",
                Status::Invalid,
                "Face encoding requires a width byte",
            ));
        }
        let face_bits = c.u8()?;
        if ![8, 16, 32].contains(&face_bits) {
            return Err(error(
                "hsf.shell_face_width",
                Status::Unsupported,
                "Only 8, 16, and 32-bit face values are supported",
            ));
        }
        let width = u32::from(face_bits / 8);
        if (length - 1) % width != 0 {
            return Err(error(
                "hsf.invalid_face_encoding",
                Status::Invalid,
                "Face byte count is not aligned to its value width",
            ));
        }
        let face_value_count = (length - 1) / width;
        charge(
            &mut self.face_values,
            face_value_count as u64,
            self.options.max_face_values,
            "hsf.limit_face_values",
        )?;
        let faces = c.array(d, face_value_count, width)?;
        let attributes = if flags & 8 != 0 {
            self.shell_attributes(d, c, vertex_count)?
        } else {
            vec![]
        };
        let normals = attributes
            .iter()
            .find(|a| a.opcode == 17)
            .map(|a| a.fields[0].source.clone());
        Ok(ShellLayout {
            null: false,
            flags,
            extended_flags,
            refinement_index,
            lod,
            vertex_count,
            vertices,
            point_quantization,
            face_value_count,
            face_bits,
            faces,
            normals,
            attributes,
            connectivity: None,
        })
    }
    /// Connectivity-compressed Shell (flag 0x40), as the reference reader frames
    /// it after the LOD: the compression scheme (5, edgebreaker), the stream's
    /// byte length and the stream, then the optional attributes. Observed only
    /// with quantized points (flag 1) in the stream.
    fn connected_shell(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
        flags: u8,
        extended_flags: u16,
        refinement_index: Option<u32>,
        lod: u8,
    ) -> Result<ShellLayout, HsfError> {
        if c.u8()? != 5 {
            return Err(error(
                "hsf.shell_connectivity_scheme",
                Status::Unsupported,
                "Only edgebreaker (scheme 5) connectivity compression is supported",
            ));
        }
        if flags & 1 == 0 {
            return Err(error(
                "hsf.shell_connectivity_points",
                Status::Unsupported,
                "Connectivity-compressed Shells without compressed points are not implemented",
            ));
        }
        let length = c.nonnegative()?;
        let at = c.pos;
        let stream = c.array(d, length, 1)?;
        let bytes = &c.data[at..c.pos];
        let sections = connectivity_sections(bytes)?;
        let vertex_count = u32::try_from(sections.points_count).map_err(|_| {
            error(
                "hsf.invalid_shell_connectivity",
                Status::Invalid,
                "Connectivity point count exceeds the index range",
            )
        })?;
        let opcode_count = sections.ops.len() as u32;
        charge(
            &mut self.vertices,
            u64::from(vertex_count),
            self.options.max_vertices,
            "hsf.limit_vertices",
        )?;
        charge(
            &mut self.face_values,
            u64::from(opcode_count) * 4,
            self.options.max_face_values,
            "hsf.limit_face_values",
        )?;
        let layout = ConnectivityLayout {
            scheme: 5,
            stream,
            stream_scheme: bytes[0],
            points_scheme: bytes[2],
            normals_scheme: bytes[3],
            opcode_count,
        };
        let attributes = if flags & 8 != 0 {
            self.shell_attributes(d, c, vertex_count)?
        } else {
            vec![]
        };
        let normals = attributes
            .iter()
            .find(|a| a.opcode == 17)
            .map(|a| a.fields[0].source.clone());
        let empty = span(d, at, 0);
        Ok(ShellLayout {
            null: false,
            flags,
            extended_flags,
            refinement_index,
            lod,
            vertex_count,
            vertices: empty.clone(),
            point_quantization: None,
            face_value_count: 0,
            face_bits: 0,
            faces: empty,
            normals,
            attributes,
            connectivity: Some(layout),
        })
    }
    /// Quantized Shell points (flag 1), in the reference reader's order: the
    /// scheme (only 1, trivial), the point count, the box the codes span (min
    /// then max XYZ), the bits per coordinate and the byte length of the codes,
    /// packed as quantized parameters are. `vertices` then spans the codes.
    fn quantized_points(
        &mut self,
        d: usize,
        c: &mut Cursor<'_>,
    ) -> Result<(u32, SourceSpan, Option<PointQuantization>), HsfError> {
        if c.u8()? != 1 {
            return Err(error(
                "hsf.shell_point_compression",
                Status::Unsupported,
                "Only trivially quantized Shell points are supported",
            ));
        }
        let vertex_count = c.nonnegative()?;
        charge(
            &mut self.vertices,
            vertex_count as u64,
            self.options.max_vertices,
            "hsf.limit_vertices",
        )?;
        let at = c.pos;
        let bounds = c.take(d, 24)?;
        progress::finite(&c.data[at..c.pos], "hsf.nonfinite_shell")?;
        let corners: Vec<f32> = c.data[at..c.pos]
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().expect("four bytes")))
            .collect();
        if (0..3).any(|i| corners[i] > corners[i + 3]) {
            return Err(error(
                "hsf.invalid_shell_point_bounds",
                Status::Invalid,
                "Quantized point minimum exceeds maximum",
            ));
        }
        let bits = c.u8()?;
        if !(2..=31).contains(&bits) {
            return Err(error(
                "hsf.shell_point_bits",
                Status::Unsupported,
                "Quantized point bit width must be 2..31",
            ));
        }
        let length = c.nonnegative()?;
        if u64::from(length) != progress::packed_length(vertex_count, 3, bits) {
            return Err(error(
                "hsf.invalid_shell_point_length",
                Status::Invalid,
                "Packed point length differs from count and bit width",
            ));
        }
        let codes = c.array(d, length, 1)?;
        Ok((
            vertex_count,
            codes,
            Some(PointQuantization {
                scheme: 1,
                bits_per_sample: bits,
                bounds,
            }),
        ))
    }
    /// HSF 20.90 writes one byte right after the opcode of bounds, cameras,
    /// views, modelling matrices, shells, clip regions and the simple
    /// primitives (markers, lines, polylines, polygons, circles, arcs and
    /// text), before their single-precision operands (`S 00 90 …` against
    /// 19.10's `S 90 …`). Only zero has been observed; any other value stops
    /// instead of guessing a double-precision layout.
    fn precision_byte(&self, c: &mut Cursor<'_>, code: &str) -> Result<(), HsfError> {
        let version = (
            self.document.header.version.major,
            self.document.header.version.minor,
        );
        if version >= (20, 90) && c.u8()? != 0 {
            return Err(error(
                code,
                Status::Unsupported,
                "Only single-precision operands are verified for HSF 20.90",
            ));
        }
        Ok(())
    }
    fn payload(&self, d: usize, c: &mut Cursor<'_>, n: u64) -> Result<SourceSpan, HsfError> {
        if n > self.options.max_string_bytes {
            return Err(error(
                "hsf.limit_string",
                Status::LimitExceeded,
                "String exceeds limit",
            ));
        }
        c.take(
            d,
            usize::try_from(n).map_err(|_| {
                error(
                    "hsf.limit_string",
                    Status::LimitExceeded,
                    "String length overflow",
                )
            })?,
        )
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}
impl Cursor<'_> {
    fn nonnegative(&mut self) -> Result<u32, HsfError> {
        let n = self.u32()?;
        if n > i32::MAX as u32 {
            return Err(error(
                "hsf.negative_count",
                Status::Invalid,
                "Negative array count",
            ));
        }
        Ok(n)
    }
    fn array(&mut self, d: usize, count: u32, width: u32) -> Result<SourceSpan, HsfError> {
        let n = usize::try_from(u64::from(count) * u64::from(width)).map_err(|_| {
            error(
                "hsf.limit_array",
                Status::LimitExceeded,
                "Array size exceeds address space",
            )
        })?;
        self.take(d, n)
    }
    fn take(&mut self, d: usize, n: usize) -> Result<SourceSpan, HsfError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| {
                error(
                    "hsf.truncated_operand",
                    Status::Invalid,
                    "Operand extends beyond its source domain",
                )
            })?;
        let result = span(d, self.pos, n);
        self.pos = end;
        Ok(result)
    }
    fn u8(&mut self) -> Result<u8, HsfError> {
        let p = self.pos;
        self.take(0, 1)?;
        Ok(self.data[p])
    }
    fn u16(&mut self) -> Result<u16, HsfError> {
        let p = self.pos;
        self.take(0, 2)?;
        Ok(u16::from_le_bytes([self.data[p], self.data[p + 1]]))
    }
    fn u32(&mut self) -> Result<u32, HsfError> {
        let p = self.pos;
        self.take(0, 4)?;
        Ok(u32::from_le_bytes(
            self.data[p..p + 4].try_into().expect("checked length"),
        ))
    }
}
