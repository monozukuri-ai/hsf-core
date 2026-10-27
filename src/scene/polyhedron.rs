use super::*;

// Independently authored wire decoder. Samples are MSB first in LE u32 words,
// except the legacy width-three/eight-bit layout, which stores direct octets.
// Interpolation uses f64 with one f32 rounding; encoded bounds remain exact.
pub(super) fn sample(bytes: &[u8], i: u64, width: u8, bits: u8, bounds: [f32; 2]) -> f32 {
    let max = (1u64 << bits) - 1;
    let code = if width == 3 && bits == 8 {
        u64::from(bytes[i as usize])
    } else {
        let pos = i * u64::from(bits);
        let at = (pos / 32 * 4) as usize;
        let offset = (pos % 32) as u32;
        let word = u64::from(u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()));
        if offset + u32::from(bits) <= 32 {
            (word >> (32 - offset - u32::from(bits))) & max
        } else {
            let tail = offset + u32::from(bits) - 32;
            let next = u64::from(u32::from_le_bytes(
                bytes[at + 4..at + 8].try_into().unwrap(),
            ));
            ((word << tail) | (next >> (32 - tail))) & max
        }
    };
    let [lo, hi] = bounds;
    if code == 0 {
        lo
    } else if code == max {
        hi
    } else {
        (f64::from(lo) + (f64::from(hi) - f64::from(lo)) * (code as f64 / max as f64)) as f32
    }
}

type NormalResult = (Option<VertexNormals>, Vec<[f32; 3]>);
pub(super) fn normals(
    doc: &HsfDocument,
    r: &Record,
    budget: &mut Budget,
) -> Result<NormalResult, HsfError> {
    let layout = if let Some(m) = &r.mesh {
        m.attributes
            .iter()
            .find(|a| a.opcode == 17)
            .map(|a| (m.vertex_count, &a.source, &a.values, None, None))
    } else {
        let s = r.shell.as_ref().expect("shell");
        s.attributes
            .iter()
            .find(|a| matches!(a.opcode, 1 | 17))
            .map(|a| {
                let field = |name| a.fields.iter().find(|f| f.name == name).map(|f| &f.source);
                let byte = |name| field(name).map(|s| doc.source_bytes(s).expect("field")[0]);
                (
                    s.vertex_count,
                    &a.source,
                    field("normals").expect("normals"),
                    byte("compression"),
                    byte("bits_per_sample"),
                )
            })
    };
    let Some((count, source, values_source, compression, bits)) = layout else {
        return Ok((None, vec![]));
    };
    budget.charge_output(2048 + u64::from(count) * 24)?;
    let bytes = doc.source_bytes(values_source).expect("framed normals");
    let (encoding, packing, bounds, values) = if let Some(bits) = bits {
        let polar = compression == Some(7);
        let width = if polar { 2 } else { 3 };
        let bounds = if polar {
            vec![
                [-std::f32::consts::PI, std::f32::consts::PI],
                [0., std::f32::consts::PI],
            ]
        } else {
            vec![[-1., 1.]; 3]
        };
        let values = (0..u64::from(count))
            .map(|i| {
                let component = |axis: usize| {
                    sample(
                        bytes,
                        i * u64::from(width) + axis as u64,
                        width,
                        bits,
                        bounds[axis],
                    )
                };
                if polar {
                    // phi is inclination from +Y; theta is azimuth atan2(z, x).
                    // Do not normalize, clamp the poles, or apply world handedness.
                    let theta = f64::from(component(0));
                    let phi = f64::from(component(1));
                    [
                        (phi.sin() * theta.cos()) as f32,
                        phi.cos() as f32,
                        (phi.sin() * theta.sin()) as f32,
                    ]
                } else {
                    std::array::from_fn(component)
                }
            })
            .collect();
        (
            if polar {
                "quantized_polar"
            } else {
                "quantized_xyz"
            },
            if width == 3 && bits == 8 {
                "u8_xyz"
            } else {
                "le32_msb"
            },
            bounds,
            values,
        )
    } else {
        (
            "f32_xyz",
            "f32_le",
            vec![],
            mesh::vectors(doc, values_source, r)?,
        )
    };
    Ok((
        Some(VertexNormals {
            count,
            source: source.clone(),
            values_source: values_source.clone(),
            encoding: encoding.into(),
            compression,
            bits_per_sample: bits,
            packing: packing.into(),
            bounds,
        }),
        values,
    ))
}

pub(super) fn metadata(
    doc: &HsfDocument,
    r: &Record,
    budget: &mut Budget,
) -> Result<Option<EntityMetadata>, HsfError> {
    let Some(a) = r
        .shell
        .as_ref()
        .and_then(|s| s.attributes.iter().find(|a| a.opcode == 99))
    else {
        return Ok(None);
    };
    budget.charge_output(2048 + a.fields.len() as u64 * 512)?;
    let fields: Result<Vec<_>, HsfError> = a
        .fields
        .iter()
        .map(|f| {
            Ok(EntityMetadataField {
                kind: f.name.clone(),
                text: primitive::text(
                    doc,
                    &f.source,
                    (f.name == "unicode_options").then_some(5),
                    budget,
                )?,
            })
        })
        .collect();
    let fields = fields?;
    let decoding_status = if fields.iter().all(|f| f.text.status == Status::Complete) {
        Status::Complete
    } else {
        Status::Unsupported
    };
    Ok(Some(EntityMetadata {
        source: a.source.clone(),
        fields,
        decoding_status,
        interpretation_status: Status::Unsupported,
    }))
}
