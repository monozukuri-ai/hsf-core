use super::*;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
pub struct TextValue {
    pub encoding: String,
    pub encoding_code: Option<u8>,
    pub source: SourceSpan,
    pub value: Option<String>,
    pub status: Status,
}

#[derive(Clone, Debug, Serialize)]
pub struct Primitive {
    pub id: String,
    /// Segment owning this definition, independent of reference placements.
    pub owner_node_id: String,
    /// `polygon` has an implicit final-to-first edge. Stored vertices are not
    /// duplicated, fitted to a plane or triangulated.
    pub kind: Cow<'static, str>,
    pub source: SourceSpan,
    pub source_record_id: Arc<str>,
    pub points_source: SourceSpan,
    pub point_count: u32,
    /// Empty for ordinary primitives; ordered component lengths for a batch.
    pub part_lengths: Vec<u32>,
    /// Explicit center only; no fitted circle, tessellation or projection.
    pub center: Option<Attribute<[f32; 3]>>,
    pub text: Option<TextValue>,
    #[serde(skip)]
    pub points: Vec<[f32; 3]>,
}

/// Colors and visibility use mask bits 0..31. Extension flags
/// (color: 7, 15, 23; visibility: 7, 23) never become geometry attributes.
///
/// A large assembly copies these per segment (copy-on-write) and per visited
/// segment (inheritance), hundreds of thousands of times: values are shared and
/// slots are stored sparsely, so a copy is a few pointers.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DisplayAttributes {
    pub text_anchor: Option<Arc<Attribute<EnumeratedValue>>>,
    pub text_justification: Option<Arc<Attribute<EnumeratedValue>>>,
    pub line_pattern: Option<Arc<Attribute<LinePattern>>>,
    pub edge_pattern: Option<Arc<Attribute<LinePattern>>>,
    pub marker_symbol: Option<Arc<Attribute<MarkerSymbol>>>,
    /// Texture transforms override inherited transforms; they never concatenate.
    pub texture_matrix: Option<Arc<Attribute<Matrix>>>,
    pub colors: Slots<[u8; 3]>,
    pub visibility: Slots<bool>,
    pub marker_size: Option<Arc<Attribute<MarkerSize>>>,
    pub line_weight: Option<Arc<Attribute<MarkerSize>>>,
    pub edge_weight: Option<Arc<Attribute<MarkerSize>>>,
}
impl DisplayAttributes {
    pub(super) fn inherit(&self, parent: &Self) -> Self {
        fn pick<T>(own: &Option<Arc<T>>, parent: &Option<Arc<T>>) -> Option<Arc<T>> {
            own.as_ref().or(parent.as_ref()).cloned()
        }
        Self {
            text_anchor: pick(&self.text_anchor, &parent.text_anchor),
            text_justification: pick(&self.text_justification, &parent.text_justification),
            line_pattern: pick(&self.line_pattern, &parent.line_pattern),
            edge_pattern: pick(&self.edge_pattern, &parent.edge_pattern),
            marker_symbol: pick(&self.marker_symbol, &parent.marker_symbol),
            texture_matrix: pick(&self.texture_matrix, &parent.texture_matrix),
            edge_weight: pick(&self.edge_weight, &parent.edge_weight),
            line_weight: pick(&self.line_weight, &parent.line_weight),
            marker_size: pick(&self.marker_size, &parent.marker_size),
            colors: self.colors.inherit(&parent.colors),
            visibility: self.visibility.inherit(&parent.visibility),
        }
    }
}

/// The 32 color or visibility slots (mask bits 0..31). Records assign one value
/// to many slots at once (a color to every geometry bit of its mask), so the set
/// slots are stored as a few (slots, value) runs, shared between clones.
/// Serialized as 32 entries, `null` for an unset slot.
#[derive(Clone, Debug)]
pub struct Slots<T> {
    set: u32,
    runs: Option<Runs<T>>,
}
/// (slots, value) pairs with disjoint slot masks.
type Runs<T> = Arc<[(u32, Attribute<T>)]>;
impl<T> Default for Slots<T> {
    fn default() -> Self {
        Self { set: 0, runs: None }
    }
}
impl<T: Clone> Slots<T> {
    pub fn get(&self, slot: usize) -> Option<&Attribute<T>> {
        let bit = 1u32.checked_shl(slot as u32)?;
        if self.set & bit == 0 {
            return None;
        }
        let runs = self.runs.as_deref()?;
        runs.iter()
            .find(|(mask, _)| mask & bit != 0)
            .map(|(_, value)| value)
    }
    /// Set bits: slot `i` is set when bit `i` is.
    pub fn mask(&self) -> u32 {
        self.set
    }
    pub fn is_empty(&self) -> bool {
        self.set == 0
    }
    /// All 32 slots in order.
    pub fn iter(&self) -> impl Iterator<Item = Option<&Attribute<T>>> {
        (0..32).map(|slot| self.get(slot))
    }
    pub fn to_array(&self) -> [Option<Attribute<T>>; 32] {
        std::array::from_fn(|slot| self.get(slot).cloned())
    }
    pub fn set(&mut self, slot: usize, value: Option<Attribute<T>>) {
        assert!(slot < 32, "attribute slot out of range");
        self.assign(1 << slot, value);
    }
    /// Give every slot in `mask` the same value, or clear them.
    pub fn assign(&mut self, mask: u32, value: Option<Attribute<T>>) {
        if mask == 0 {
            return;
        }
        let mut runs: Vec<(u32, Attribute<T>)> = self
            .runs
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|(m, _)| m & !mask != 0)
            .map(|(m, v)| (m & !mask, v.clone()))
            .collect();
        if let Some(value) = value {
            runs.push((mask, value));
        }
        self.set = runs.iter().fold(0, |set, (m, _)| set | m);
        self.runs = (!runs.is_empty()).then(|| Arc::from(runs));
    }
    /// Own slots win; the parent fills the rest. Shares either side when the
    /// other adds nothing.
    fn inherit(&self, parent: &Self) -> Self {
        let missing = parent.set & !self.set;
        if missing == 0 {
            return self.clone();
        }
        if self.set == 0 {
            return parent.clone();
        }
        let own = self.runs.as_deref().unwrap_or_default().iter().cloned();
        let inherited = parent
            .runs
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|(m, _)| m & missing != 0)
            .map(|(m, v)| (m & missing, v.clone()));
        let runs: Vec<_> = own.chain(inherited).collect();
        Self {
            set: self.set | missing,
            runs: Some(Arc::from(runs)),
        }
    }
}
impl<T: Clone + Serialize> Serialize for Slots<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct PrimitiveInstance {
    pub world_handedness: Option<Attribute<Handedness>>,
    pub id: String,
    pub primitive_id: String,
    pub node_id: String,
    /// Node and reference ids from the root, shared like `Instance::path`.
    pub path: Arc<[Arc<str>]>,
    pub world_transform: Matrix,
    pub determinant: f64,
    pub display: Arc<DisplayAttributes>,
}
#[derive(Clone, Debug, Serialize)]
pub struct UserOptions {
    pub node_id: String,
    pub source_record_id: Arc<str>,
    pub text: TextValue,
}

pub(super) fn text(
    doc: &HsfDocument,
    source: &SourceSpan,
    code: Option<u8>,
    budget: &mut Budget,
) -> Result<TextValue, HsfError> {
    let bytes = doc.source_bytes(source).expect("framed text");
    // Worst-case UTF-16 -> UTF-8 expansion plus transient code units. Charged
    // before any decoded allocation, also for unsupported encodings.
    budget.charge_output(512 + bytes.len() as u64 * 4)?;
    let invalid = || {
        error(
            "hsf.invalid_text_encoding",
            Status::Invalid,
            "Malformed Unicode text; replacement characters are not inserted",
        )
    };
    let (encoding, value) = match code {
        None if bytes.is_ascii() => (
            "ascii",
            Some(String::from_utf8(bytes.to_vec()).expect("ASCII")),
        ),
        None => ("unspecified", None),
        Some(0) => (
            "iso_8859_1",
            Some(bytes.iter().map(|b| char::from(*b)).collect()),
        ),
        Some(5) => {
            if bytes.len() % 2 != 0 {
                return Err(invalid());
            }
            let value: Result<String, _> = char::decode_utf16(
                bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]])),
            )
            .collect();
            ("utf_16_le", Some(value.map_err(|_| invalid())?))
        }
        Some(6) => {
            if bytes.len() % 4 != 0 {
                return Err(invalid());
            }
            let value: Option<String> = bytes
                .chunks_exact(4)
                .map(|b| char::from_u32(u32::from_le_bytes(b.try_into().expect("four"))))
                .collect();
            ("utf_32_le", Some(value.ok_or_else(invalid)?))
        }
        // UTF-8 is a distinct encoding, not a fallback for an unspecified or
        // unknown code. Code 10 is verified by the native eDrawings 2026 export
        // of the independently authored Japanese fixture.
        Some(10) => (
            "utf_8",
            Some(
                std::str::from_utf8(bytes)
                    .map_err(|_| invalid())?
                    .to_owned(),
            ),
        ),
        _ => ("unsupported", None),
    };
    Ok(TextValue {
        encoding: encoding.into(),
        encoding_code: code,
        source: source.clone(),
        status: if value.is_some() {
            Status::Complete
        } else {
            Status::Unsupported
        },
        value,
    })
}

fn point(bytes: &[u8]) -> Result<[f32; 3], HsfError> {
    let result = std::array::from_fn(|i| {
        f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("four"))
    });
    if result.iter().any(|v| !v.is_finite()) {
        return Err(error(
            "hsf.invalid_primitive_point",
            Status::Invalid,
            "Primitive coordinates must be finite",
        ));
    }
    Ok(result)
}
pub(super) fn decode(
    doc: &HsfDocument,
    r: &Record,
    budget: &mut Budget,
) -> Result<Primitive, HsfError> {
    if let Some(l) = &r.poly_polyline() {
        budget.charge_output(
            2048 + u64::from(l.point_count) * 24 + u64::from(l.primitive_count) * 8,
        )?;
        let lengths: Vec<u32> = doc
            .source_bytes(&l.lengths)
            .expect("framed lengths")
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let mut values = doc
            .source_bytes(&l.points)
            .expect("framed points")
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()));
        let axes: [u16; 3] = std::array::from_fn(|a| (l.flags >> (8 + 2 * a)) & 3);
        let extra = doc.source_bytes(&l.leftovers).expect("framed axis values");
        let mut offset = 0usize;
        let axis_values: [&[u8]; 3] = std::array::from_fn(|a| {
            let count = match axes[a] {
                2 => 1,
                3 => l.primitive_count as usize,
                _ => 0,
            };
            let range = &extra[offset..offset + count * 4];
            offset += count * 4;
            range
        });
        let mut points = Vec::with_capacity(l.point_count as usize);
        for (part, length) in lengths.iter().enumerate() {
            for _ in 0..*length {
                points.push(std::array::from_fn(|axis| match axes[axis] {
                    0 => values.next().expect("framed coordinate"),
                    1 => 0.,
                    mode => {
                        let at = if mode == 2 { 0 } else { part * 4 };
                        f32::from_le_bytes(axis_values[axis][at..at + 4].try_into().unwrap())
                    }
                }));
            }
        }
        return Ok(Primitive {
            id: String::new(),
            owner_node_id: String::new(),
            kind: r.kind.clone(),
            source: r.source.clone(),
            source_record_id: r.id.clone(),
            points_source: l.points.clone(),
            point_count: l.point_count,
            part_lengths: lengths,
            center: None,
            text: None,
            points,
        });
    }
    let layout = r.primitive().expect("primitive framing");
    budget.charge_output(512 + u64::from(layout.point_count) * 12)?;
    let points = doc
        .source_bytes(&layout.points)
        .expect("framed points")
        .chunks_exact(12)
        .map(point)
        .collect::<Result<Vec<_>, _>>()?;
    let center = layout
        .center
        .as_ref()
        .map(|s| {
            point(doc.source_bytes(s).expect("center")).map(|value| Attribute {
                value,
                source: s.clone(),
            })
        })
        .transpose()?;
    let text = layout
        .text
        .as_ref()
        .map(|s| text(doc, s, layout.encoding, budget))
        .transpose()?;
    Ok(Primitive {
        id: String::new(),
        owner_node_id: String::new(),
        kind: r.kind.clone(),
        source: r.source.clone(),
        source_record_id: r.id.clone(),
        points_source: layout.points.clone(),
        point_count: layout.point_count,
        part_lengths: vec![],
        center,
        text,
        points,
    })
}
