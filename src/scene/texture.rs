//! Explicit diffuse texture requests and bounded local-image/Style associations.
//! This is not material evaluation: assignments are ordered events, not net state.
use super::*;

/// Decoded stored options only; missing values are not runtime defaults.
#[derive(Clone, Debug, Serialize)]
pub struct TextureOptions {
    pub enumerations: Vec<TextureEnum>,
    pub value_scale: Option<Attribute<[f32; 2]>>,
    pub transform_segment: Option<TextValue>,
    pub parameter_offset: Option<Attribute<u8>>,
    /// Flags whose effects/numeric meanings are not evaluated by this profile.
    pub unevaluated_flags: u32,
    pub decoding_status: Status,
}
#[derive(Clone, Debug, Serialize)]
pub struct TextureEnum {
    pub name: String,
    pub raw: u8,
    pub value: Option<String>,
    pub source: SourceSpan,
}

pub(super) fn decode_options(
    doc: &HsfDocument,
    layout: &TextureLayout,
    budget: &mut Budget,
) -> Result<TextureOptions, HsfError> {
    budget.charge_output(8192)?;
    let mut out = TextureOptions {
        enumerations: vec![],
        value_scale: None,
        transform_segment: None,
        parameter_offset: None,
        unevaluated_flags: layout.flags & !(0x0fff | 0x8000 | 0x40000),
        decoding_status: Status::Complete,
    };
    for f in &layout.fields {
        let bytes = doc.source_bytes(&f.source).expect("texture operand");
        match f.name.as_str() {
            "value_scale" => {
                out.value_scale = Some(Attribute {
                    value: std::array::from_fn(|i| {
                        f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                    }),
                    source: f.source.clone(),
                })
            }
            "transform_segment" => {
                out.transform_segment = Some(primitive::text(doc, &f.source, None, budget)?)
            }
            "parameter_offset" => {
                out.parameter_offset = Some(Attribute {
                    value: bytes[0],
                    source: f.source.clone(),
                })
            }
            name => {
                // Public opcode tables specify these numeric mappings. The
                // extended apply_mode bit values are deliberately left raw.
                let names: &[&str] = match name {
                    "parameter_source" => &[
                        "u",
                        "uv",
                        "uvw",
                        "object",
                        "world",
                        "surface_normal",
                        "reflection_vector",
                        "natural_uv",
                        "local_pixels",
                        "outer_pixels",
                        "local_window",
                        "outer_window",
                    ],
                    "tiling" => &["none", "clamp", "repeat", "mirror"],
                    "interpolation" | "decimation" => &[
                        "none",
                        "bilinear",
                        "trilinear",
                        "mipmap",
                        "summed_areas",
                        "gaussian",
                        "stochastic",
                    ],
                    "red_mapping" | "green_mapping" | "blue_mapping" | "alpha_mapping" => &[
                        "red",
                        "green",
                        "blue",
                        "alpha",
                        "zero",
                        "one",
                        "luminance",
                        "none",
                    ],
                    "parameter_function" => &["none", "sphere", "cylinder", "box"],
                    "layout" => &["rectilinear", "spherical", "hemispherical", "cubic_faces"],
                    _ => &[],
                };
                let value = names.get(usize::from(bytes[0])).map(|s| (*s).into());
                if value.is_none() {
                    out.decoding_status = Status::Unsupported;
                }
                out.enumerations.push(TextureEnum {
                    name: name.into(),
                    raw: bytes[0],
                    value,
                    source: f.source.clone(),
                });
            }
        }
    }
    if out.unevaluated_flags != 0 {
        out.decoding_status = Status::Unsupported;
    }
    Ok(out)
}

#[derive(Clone, Debug, Serialize)]
pub struct TextureUse {
    pub id: String,
    pub node_id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub geometry_mask: u32,
    pub specification: TextValue,
    pub texture_name: Option<TextValue>,
    /// Stored normalized RGB, not quantized RGB8 or a composited material color.
    pub diffuse_color: Option<Attribute<[f32; 3]>>,
    pub binding: TextureBinding,
    pub interpretation_status: Status,
}

#[derive(Clone, Debug, Serialize)]
pub struct TextureBinding {
    /// Always local_and_styles. No physical-parent or Include-path fallback.
    pub scope: String,
    /// Complete means a unique local implicit image association, not rendering.
    pub status: Status,
    pub reason: Option<String>,
    pub image_id: Option<String>,
    /// Same-owner, same-name explicit options referring to that same image.
    /// Definition scope, override order and option effects remain unevaluated.
    pub texture_definition_ids: Vec<String>,
    /// Outer-to-inner Style references used to reach the image owner.
    pub style_path: Vec<String>,
}
impl TextureBinding {
    fn unresolved(reason: &str) -> Self {
        Self {
            scope: "local_and_styles".into(),
            status: Status::Unsupported,
            reason: Some(reason.into()),
            image_id: None,
            texture_definition_ids: vec![],
            style_path: vec![],
        }
    }
}

struct ParsedRequest {
    name: std::ops::Range<usize>,
    color: Option<([f32; 3], std::ops::Range<usize>)>,
}
struct TextCursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl TextCursor<'_> {
    fn spaces(&mut self) {
        while self.bytes.get(self.at) == Some(&b' ') {
            self.at += 1;
        }
    }
    fn token(&mut self, value: &[u8]) -> Option<()> {
        self.spaces();
        let end = self.at.checked_add(value.len())?;
        if !self.bytes.get(self.at..end)?.eq_ignore_ascii_case(value) {
            return None;
        }
        self.at = end;
        Some(())
    }
    fn diffuse(&mut self, field: &[u8]) -> Option<()> {
        self.token(b"diffuse")?;
        if self.bytes.get(self.at) != Some(&b' ') {
            return None;
        }
        self.token(field)?;
        self.token(b"=")
    }
}
fn request(bytes: &[u8]) -> Option<ParsedRequest> {
    if bytes.len() > 4096 || !bytes.iter().all(|b| (32..127).contains(b)) {
        return None;
    }
    let mut c = TextCursor { bytes, at: 0 };
    c.spaces();
    let start = c.at;
    let color = if c.diffuse(b"color").is_some() {
        c.spaces();
        let close = match *bytes.get(c.at)? {
            b'`' => b'`',
            b'(' => b')',
            _ => return None,
        };
        c.at += 1;
        c.spaces();
        let rgb_start = c.at;
        let mut rgb = [0.; 3];
        for (i, channel) in [b"r".as_slice(), b"g", b"b"].into_iter().enumerate() {
            c.token(channel)?;
            c.token(b"=")?;
            c.spaces();
            let number_start = c.at;
            while bytes
                .get(c.at)
                .is_some_and(|b| b.is_ascii_digit() || b"+-.eE".contains(b))
            {
                c.at += 1;
            }
            rgb[i] = std::str::from_utf8(&bytes[number_start..c.at])
                .ok()?
                .parse::<f32>()
                .ok()?;
            if !rgb[i].is_finite() || !(0.0..=1.0).contains(&rgb[i]) {
                return None;
            }
            if i < 2 && bytes.get(c.at) != Some(&b' ') {
                return None;
            }
        }
        let rgb_end = c.at;
        c.token(&[close])?;
        c.token(b",")?;
        Some((rgb, rgb_start..rgb_end))
    } else {
        c.at = start;
        None
    };
    c.diffuse(b"texture")?;
    c.spaces();
    let parens = bytes.get(c.at) == Some(&b'(');
    if parens {
        c.at += 1;
        c.spaces();
    }
    let name_start = c.at;
    while bytes
        .get(c.at)
        .is_some_and(|b| b.is_ascii_alphanumeric() || b"_-.".contains(b))
    {
        c.at += 1;
    }
    if c.at == name_start {
        return None;
    }
    let name = name_start..c.at;
    if parens {
        c.token(b")")?;
    }
    c.spaces();
    if c.at != bytes.len() {
        return None;
    }
    Some(ParsedRequest { name, color })
}
fn subspan(source: &SourceSpan, range: std::ops::Range<usize>) -> SourceSpan {
    SourceSpan {
        domain_id: source.domain_id.clone(),
        offset: source.offset + range.start as u64,
        length: (range.end - range.start) as u64,
    }
}

struct Index {
    images: Vec<Vec<usize>>,
    textures: Vec<Vec<usize>>,
}
struct Found {
    image: usize,
    definitions: Vec<usize>,
    path: Vec<usize>,
}
enum Local {
    Found(Found),
    Missing,
    Blocked(&'static str),
}
struct Frame {
    node: usize,
    next: usize,
    prepared: bool,
    incoming: Option<usize>,
    found: Option<Found>,
    reason: Option<&'static str>,
}

impl Builder<'_> {
    pub(super) fn texture_use(&mut self, r: &Record, n: usize) -> Result<(), HsfError> {
        let layout = r.rendering.as_ref().expect("framed color");
        let Some(field) = layout
            .fields
            .iter()
            .find(|f| f.name == "diffuse" && f.encoding == "unspecified_text")
        else {
            return Ok(());
        };
        let bytes = self.doc.source_bytes(&field.source).expect("framed field");
        // Detect an explicit texture keyword, then parse the WHOLE string. Other
        // color forms remain in rendering_options, not inferred as texture names.
        let has_keyword = bytes.windows(7).enumerate().any(|(i, b)| {
            b.eq_ignore_ascii_case(b"texture")
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
                && (i + 7 == bytes.len() || !bytes[i + 7].is_ascii_alphanumeric())
        });
        if !has_keyword {
            return Ok(());
        }
        self.budget.charge_output(4096)?;
        let parsed = request(bytes);
        let specification = primitive::text(self.doc, &field.source, None, self.budget)?;
        let (texture_name, diffuse_color) = if let Some(p) = parsed {
            let name_source = subspan(&field.source, p.name);
            (
                Some(primitive::text(self.doc, &name_source, None, self.budget)?),
                p.color.map(|(value, range)| Attribute {
                    value,
                    source: subspan(&field.source, range),
                }),
            )
        } else {
            (None, None)
        };
        let reason = if texture_name.is_some() {
            "hsf.texture_not_resolved"
        } else {
            "hsf.texture_expression"
        };
        self.data.texture_uses.push(TextureUse {
            id: format!("texture_use:{}", self.data.texture_uses.len()),
            node_id: self.data.nodes[n].id.clone(),
            source_record_id: r.id.clone(),
            source: r.source.clone(),
            geometry_mask: layout.mask as u32 & 0x017f7f7f,
            specification,
            texture_name,
            diffuse_color,
            binding: TextureBinding::unresolved(reason),
            interpretation_status: Status::Unsupported,
        });
        Ok(())
    }

    pub(super) fn resolve_texture_uses(&mut self, complete_prefix: bool) -> Result<(), HsfError> {
        self.texture_uses_inner(complete_prefix).map_err(|mut e| {
            if e.diagnostic.stage == "hsf_header" {
                e.diagnostic.stage = "scene".into();
            }
            e.diagnostic.version = Some(self.doc.header.version.text.clone());
            e
        })
    }
    fn texture_uses_inner(&mut self, complete_prefix: bool) -> Result<(), HsfError> {
        if self.data.texture_uses.is_empty() {
            return Ok(());
        }
        if !complete_prefix {
            for usage in &mut self.data.texture_uses {
                self.budget.charge_output(128)?;
                usage.binding = TextureBinding::unresolved("hsf.texture_incomplete_scene");
            }
            return Ok(());
        }
        self.budget.charge_output(
            self.data.nodes.len() as u64 * 512
                + (self.data.images.len() + self.data.textures.len()) as u64 * 64,
        )?;
        let by_id: HashMap<String, usize> = self
            .data
            .nodes
            .iter()
            .enumerate()
            .map(|(n, x)| (x.id.clone(), n))
            .collect();
        let mut index = Index {
            images: vec![vec![]; self.data.nodes.len()],
            textures: vec![vec![]; self.data.nodes.len()],
        };
        for (i, x) in self.data.images.iter().enumerate() {
            index.images[by_id[&x.node_id]].push(i);
        }
        for (i, x) in self.data.textures.iter().enumerate() {
            index.textures[by_id[&x.context_node_id]].push(i);
        }
        for u in 0..self.data.texture_uses.len() {
            let usage = &self.data.texture_uses[u];
            let Some(name) = &usage.texture_name else {
                continue;
            };
            self.budget.charge_output(4096 + name.source.length)?;
            let name = self
                .doc
                .source_bytes(&name.source)
                .expect("name span")
                .to_vec();
            let owner = by_id[&usage.node_id];
            self.data.texture_uses[u].binding = match self.texture_lookup(owner, &name, &index)? {
                Local::Found(mut found) => {
                    self.budget.charge_output(
                        1024 + found.path.len() as u64 * 128 + found.definitions.len() as u64 * 128,
                    )?;
                    found.path.reverse();
                    TextureBinding {
                        scope: "local_and_styles".into(),
                        status: Status::Complete,
                        reason: None,
                        image_id: Some(self.data.images[found.image].id.clone()),
                        texture_definition_ids: found
                            .definitions
                            .iter()
                            .map(|i| self.data.textures[*i].id.clone())
                            .collect(),
                        style_path: found
                            .path
                            .iter()
                            .map(|i| self.data.style_segments[*i].id.clone())
                            .collect(),
                    }
                }
                Local::Blocked(reason) => TextureBinding::unresolved(reason),
                Local::Missing => TextureBinding::unresolved("hsf.texture_parent_context"),
            };
        }
        Ok(())
    }
    fn texture_visit(&mut self) -> Result<(), HsfError> {
        self.visits += 1;
        limit(
            self.visits,
            self.options.max_visits,
            "hsf.limit_scene_visits",
        )
    }
    fn texture_local(&mut self, n: usize, name: &[u8], index: &Index) -> Result<Local, HsfError> {
        let mut image = None;
        for i in &index.images[n] {
            self.texture_visit()?;
            let resource = &self.data.images[*i];
            let candidate = self
                .doc
                .source_bytes(&resource.name.source)
                .expect("image name");
            if candidate != name && candidate.eq_ignore_ascii_case(name) {
                return Ok(Local::Blocked("hsf.texture_name_semantics"));
            }
            if candidate != name {
                continue;
            }
            if !resource.implicit_texture_local {
                return Ok(Local::Blocked("hsf.texture_image_scope"));
            }
            if image.replace(*i).is_some() {
                return Ok(Local::Blocked("hsf.texture_ambiguous_image"));
            }
        }
        let mut definition = None;
        for i in &index.textures[n] {
            self.texture_visit()?;
            let resource = &self.data.textures[*i];
            let candidate = self
                .doc
                .source_bytes(&resource.name.source)
                .expect("texture name");
            if candidate != name && candidate.eq_ignore_ascii_case(name) {
                return Ok(Local::Blocked("hsf.texture_name_semantics"));
            }
            if candidate != name {
                continue;
            }
            if definition.replace(*i).is_some() {
                return Ok(Local::Blocked("hsf.texture_ambiguous_definition"));
            }
            if self
                .doc
                .source_bytes(&resource.image.source)
                .expect("image source")
                != name
            {
                return Ok(Local::Blocked("hsf.texture_definition_source"));
            }
        }
        if let Some(image) = image {
            self.budget.charge_output(256)?;
            Ok(Local::Found(Found {
                image,
                definitions: definition.into_iter().collect(),
                path: vec![],
            }))
        } else if definition.is_some() {
            Ok(Local::Blocked("hsf.texture_image_scope"))
        } else {
            Ok(Local::Missing)
        }
    }
    fn texture_lookup(
        &mut self,
        owner: usize,
        name: &[u8],
        index: &Index,
    ) -> Result<Local, HsfError> {
        let mut frames = vec![];
        self.texture_frame(owner, None, &mut frames)?;
        loop {
            let n = frames.last().expect("active lookup").node;
            if !frames.last().expect("frame").prepared {
                let local = self.texture_local(n, name, index)?;
                let f = frames.last_mut().expect("frame");
                f.prepared = true;
                match local {
                    Local::Found(found) => {
                        f.found = Some(found);
                        f.next = self.style_edges[n].len();
                    }
                    Local::Blocked(reason) => f.reason = Some(reason),
                    Local::Missing => {}
                }
            }
            let f = frames.last_mut().expect("frame");
            if f.reason.is_none() && f.next < self.style_edges[n].len() {
                let (style, target) = self.style_edges[n][f.next];
                f.next += 1;
                self.texture_visit()?;
                let reason = if self.data.style_segments[style].condition.is_some() {
                    Some("hsf.texture_style_condition")
                } else if target.is_none() {
                    Some("hsf.texture_style_unresolved")
                } else if frames.iter().any(|f| Some(f.node) == target) {
                    Some("hsf.texture_style_cycle")
                } else {
                    None
                };
                if let Some(reason) = reason {
                    frames.last_mut().expect("frame").reason = Some(reason);
                } else {
                    self.texture_frame(target.expect("checked target"), Some(style), &mut frames)?;
                }
                continue;
            }
            let mut f = frames.pop().expect("frame");
            if let Some(found) = &mut f.found {
                if let Some(style) = f.incoming {
                    self.budget.charge_output(128)?;
                    found.path.push(style);
                }
            }
            if let Some(parent) = frames.last_mut() {
                if let Some(reason) = f.reason {
                    parent.reason = Some(reason);
                } else if let Some(found) = f.found {
                    if parent.found.is_some() {
                        parent.reason = Some("hsf.texture_style_conflict");
                    } else {
                        parent.found = Some(found);
                    }
                }
            } else {
                return Ok(if let Some(reason) = f.reason {
                    Local::Blocked(reason)
                } else if let Some(found) = f.found {
                    Local::Found(found)
                } else {
                    Local::Missing
                });
            }
        }
    }
    fn texture_frame(
        &mut self,
        node: usize,
        incoming: Option<usize>,
        frames: &mut Vec<Frame>,
    ) -> Result<(), HsfError> {
        limit(
            frames.len() as u64 + 1,
            self.options.max_depth,
            "hsf.limit_scene_depth",
        )?;
        self.budget.charge_output(512)?;
        frames.push(Frame {
            node,
            incoming,
            next: 0,
            prepared: false,
            found: None,
            reason: None,
        });
        Ok(())
    }
}
