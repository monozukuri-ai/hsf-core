use super::*;

// Shared decoder for full definitions and source-backed refinement parameters.
// Quantization is reconstructed in f64 and rounded once to f32, retaining the
// bounds and packed source. Original pre-quantization values cannot be recovered.
pub(super) fn parameters(
    doc: &HsfDocument,
    r: &Record,
    budget: &mut Budget,
) -> Result<(Option<VertexParameters>, Vec<f32>), HsfError> {
    let layout = if let Some(m) = &r.mesh {
        m.attributes
            .iter()
            .find(|a| matches!(a.opcode, 3 | 27))
            .map(|a| {
                (
                    a.width,
                    m.vertex_count,
                    &a.source,
                    &a.values,
                    a.bits_per_sample,
                    a.bounds.as_ref(),
                )
            })
    } else {
        let s = r.shell.as_ref().expect("shell");
        s.attributes
            .iter()
            .find(|a| matches!(a.opcode, 3 | 27))
            .map(|a| {
                let field = |name| a.fields.iter().find(|f| f.name == name).map(|f| &f.source);
                let byte = |name| field(name).map(|v| doc.source_bytes(v).expect("field")[0]);
                (
                    byte("width").unwrap_or(3),
                    s.vertex_count,
                    &a.source,
                    field("parameters").expect("parameters"),
                    byte("bits_per_sample"),
                    field("bounds"),
                )
            })
    };
    let Some((width, count, source, values_source, bits, bounds_source)) = layout else {
        return Ok((None, vec![]));
    };
    let len = u64::from(count) * u64::from(width);
    // Includes temporary and retained per-component quantization bounds even
    // when there are no vertices. Width is not constrained to XYZ triples.
    budget.charge_output(2048 + len * 8 + u64::from(width) * 24)?;
    let bytes = doc.source_bytes(values_source).expect("parameters");
    let (quantization, values) = if let Some(bits) = bits {
        let bounds_source = bounds_source.expect("quantization bounds");
        let raw = doc.source_bytes(bounds_source).expect("bounds");
        let floats: Vec<f32> = raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let bounds: Vec<[f32; 2]> = (0..usize::from(width))
            .map(|i| [floats[i], floats[i + usize::from(width)]])
            .collect();
        if bounds.iter().any(|b| b[0] > b[1]) {
            return Err(fail(
                r,
                "hsf.invalid_parameter_bounds",
                Status::Invalid,
                "Parameter minimum exceeds maximum",
            ));
        }
        let legacy = width == 3 && bits == 8;
        let values = (0..len)
            .map(|i| {
                polyhedron::sample(
                    bytes,
                    i,
                    width,
                    bits,
                    bounds[i as usize % usize::from(width)],
                )
            })
            .collect();
        (
            Some(ParameterQuantization {
                bits_per_sample: bits,
                packing: if legacy { "u8_xyz" } else { "le32_msb" }.into(),
                bounds,
                bounds_source: bounds_source.clone(),
            }),
            values,
        )
    } else {
        (
            None,
            bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect(),
        )
    };
    Ok((
        Some(VertexParameters {
            width,
            count,
            source: source.clone(),
            values_source: values_source.clone(),
            quantization,
        }),
        values,
    ))
}

impl Builder<'_> {
    pub(super) fn refine_shell(&mut self, r: &Record, n: usize) -> Result<(), HsfError> {
        let s = r.shell.as_ref().expect("shell");
        let parameter_index = self.data.shell_parameter_sets.len();
        self.shell_parameters(r, n)?;
        let target = s
            .refinement_index
            .and_then(|index| self.tags.get(&index))
            .cloned();
        let target = match (s.lod, target) {
            (0, Some(Object::NullShell(i))) if self.data.null_shells[i].lod == 0 => i,
            _ => {
                self.unresolved_refinement = true;
                self.issue(
                    r,
                    "hsf.shell_lod",
                    "Only LOD 0 refinement of a prior LOD 0 Null Shell is applied",
                )?;
                return Ok(());
            }
        };
        if let Some(previous) = self.null_meshes.get(&target).copied() {
            self.unresolved_refinement = true;
            self.null_meshes.insert(target, None);
            self.data.null_shells[target].mesh_id = None;
            if let Some(i) = previous {
                let mesh = &mut self.data.meshes[i];
                let refinement = mesh.refinement.as_mut().expect("refinement");
                refinement.application_status = Status::Unsupported;
                refinement.application_reason = Some("hsf.refinement_duplicate".into());
                if let Some(p) = self.refinement_parameters.get(&target) {
                    self.data.shell_parameter_sets[*p].interpretation_status = Status::Unsupported;
                }
            }
            self.issue(r, "hsf.refinement_duplicate", "Multiple refinements of one Null Shell are not selected; geometry binding is revoked")?;
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
        self.budget.charge_output(2048)?;
        let attributes_supported = s
            .attributes
            .iter()
            .all(|a| [1, 3, 17, 27, 99].contains(&a.opcode));
        if !attributes_supported {
            self.issue(
                r,
                "hsf.mesh_attributes",
                "Refinement geometry is decoded; optional attributes remain unsupported",
            )?;
            self.budget.charge_output(2048)?;
            self.data.rendering_options.push(RenderingOptions {
                line_style_binding: None,
                glyph_binding: None,
                settings: None,
                node_id: self.data.null_shells[target].node_id.clone(),
                source_record_id: r.id.clone(),
                source: r.source.clone(),
                kind: "shell".into(),
                interpretation_status: Status::Unsupported,
            });
        }
        let (mut mesh, unsupported) =
            decode(self.doc, r, self.options, &mut self.triangles, self.budget)?;
        if let Some(reason) = unsupported {
            self.issue(r, "hsf.shell_triangulation", reason)?;
        }
        let i = self.data.meshes.len();
        self.budget
            .charge_output(128 + self.data.null_shells[target].node_id.len() as u64)?;
        mesh.owner_node_id = self.data.null_shells[target].node_id.clone();
        mesh.id = format!("refinement_mesh:{}", self.refinement_count);
        self.refinement_count += 1;
        mesh.refinement = Some(ShellRefinement {
            index: s.refinement_index.expect("refinement"),
            lod: s.lod,
            context_node_id: self.data.nodes[n].id.clone(),
            target_id: self.data.null_shells[target].id.clone(),
            target_node_id: self.data.null_shells[target].node_id.clone(),
            application_status: Status::Complete,
            application_reason: None,
            attribute_status: if attributes_supported {
                Status::Complete
            } else {
                Status::Unsupported
            },
        });
        if let Some(p) = self.data.shell_parameter_sets.get_mut(parameter_index) {
            self.refinement_parameters.insert(target, parameter_index);
            p.mesh_id = Some(mesh.id.clone());
            p.interpretation_status = if attributes_supported {
                Status::Complete
            } else {
                Status::Unsupported
            };
        }
        self.data.null_shells[target].mesh_id = Some(mesh.id.clone());
        self.null_meshes.insert(target, Some(i));
        self.data.meshes.push(mesh);
        // Refinement updates the original object, not the current segment.
        self.last = Some(Object::NullShell(target));
        Ok(())
    }

    pub(super) fn shell_parameters(&mut self, r: &Record, n: usize) -> Result<(), HsfError> {
        let s = r.shell.as_ref().expect("shell");
        if !s.attributes.iter().any(|a| matches!(a.opcode, 3 | 27)) {
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
        self.budget.charge_output(2048)?;
        let (metadata, parameters) = parameters(self.doc, r, self.budget)?;
        let (target_id, reason) = match s.refinement_index {
            None => (None, Some("hsf.parameter_nonzero_lod")),
            Some(index) => match self.tags.get(&index) {
                Some(Object::Mesh(i, _)) => (Some(self.data.meshes[*i].id.clone()), None),
                Some(Object::NullShell(i)) => (Some(self.data.null_shells[*i].id.clone()), None),
                Some(_) => (None, Some("hsf.parameter_target_kind")),
                None => (None, Some("hsf.parameter_target_unresolved")),
            },
        };
        self.data.shell_parameter_sets.push(ShellParameterSet {
            mesh_id: None,
            id: format!("shell_parameters:{}", self.data.shell_parameter_sets.len()),
            node_id: self.data.nodes[n].id.clone(),
            source_record_id: r.id.clone(),
            source: r.source.clone(),
            vertices_source: s.vertices.clone(),
            refinement_index: s.refinement_index,
            lod: s.lod,
            association_status: if reason.is_none() {
                Status::Complete
            } else {
                Status::Unsupported
            },
            association_reason: reason.map(str::to_owned),
            target_id,
            vertex_parameters: metadata.expect("parameter attribute"),
            parameters,
            interpretation_status: Status::Unsupported,
        });
        Ok(())
    }
}

pub(super) fn vectors(
    doc: &HsfDocument,
    span: &SourceSpan,
    r: &Record,
) -> Result<Vec<[f32; 3]>, HsfError> {
    doc.source_bytes(span)
        .expect("framed span")
        .chunks_exact(12)
        .map(|v| {
            let a = std::array::from_fn(|i| {
                f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().expect("four bytes"))
            });
            if a.iter().any(|v| !v.is_finite()) {
                Err(fail(
                    r,
                    "hsf.nonfinite_geometry",
                    Status::Invalid,
                    "Shell contains a non-finite coordinate or normal",
                ))
            } else {
                Ok(a)
            }
        })
        .collect()
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f64; 3] {
    std::array::from_fn(|i| f64::from(a[i]) - f64::from(b[i]))
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn convex_quad(v: &[[f32; 3]], ids: &[i32]) -> bool {
    let p: [[f32; 3]; 4] = std::array::from_fn(|i| v[ids[i] as usize]);
    let normal = cross(sub(p[1], p[0]), sub(p[2], p[0]));
    let norm = dot(normal, normal).sqrt();
    let scale = (1..4)
        .map(|i| {
            let d = sub(p[i], p[0]);
            dot(d, d).sqrt()
        })
        .fold(0., f64::max);
    if norm == 0. || dot(normal, sub(p[3], p[0])).abs() > norm * scale * 1e-6 {
        return false;
    }
    (0..4).all(|i| {
        dot(
            normal,
            cross(
                sub(p[(i + 1) % 4], p[i]),
                sub(p[(i + 2) % 4], p[(i + 1) % 4]),
            ),
        ) > 0.
    })
}

pub(super) fn decode(
    doc: &HsfDocument,
    r: &Record,
    options: &SceneOptions,
    count: &mut u64,
    budget: &mut Budget,
) -> Result<(Mesh, Option<&'static str>), HsfError> {
    if r.mesh.is_some() {
        return grid(doc, r, options, count, budget).map(|m| (m, None));
    }
    let s = r.shell.as_ref().expect("framed shell");
    // Includes vector capacity growth and retained metadata before allocation.
    budget
        .charge_output(2048 + u64::from(s.vertex_count) * 24 + u64::from(s.face_value_count) * 8)?;
    let vertices = vectors(doc, &s.vertices, r)?;
    let (vertex_normals, normals) = polyhedron::normals(doc, r, budget)?;
    let entity_metadata = polyhedron::metadata(doc, r, budget)?;
    let signed = s.extended_flags & 4 != 0;
    let face_list: Vec<i32> = doc
        .source_bytes(&s.faces)
        .expect("span")
        .chunks_exact(usize::from(s.face_bits / 8))
        .map(|b| {
            let value = match (s.face_bits, signed) {
                (8, true) => i64::from(b[0] as i8),
                (8, false) => i64::from(b[0]),
                (16, true) => i64::from(i16::from_le_bytes(b.try_into().expect("two"))),
                (16, false) => i64::from(u16::from_le_bytes(b.try_into().expect("two"))),
                (32, true) => i64::from(i32::from_le_bytes(b.try_into().expect("four"))),
                (32, false) => i64::from(u32::from_le_bytes(b.try_into().expect("four"))),
                _ => unreachable!("verified width"),
            };
            i32::try_from(value).map_err(|_| {
                fail(
                    r,
                    "hsf.invalid_face_value",
                    Status::Invalid,
                    "Face value exceeds signed index range",
                )
            })
        })
        .collect::<Result<_, _>>()?;
    let mut triangles = vec![];
    let mut triangle_faces = vec![];
    let mut unsupported = None;
    let mut offset = 0;
    let mut face = 0;
    let strips = s.flags & 4 != 0;
    while offset < face_list.len() {
        let value = face_list[offset];
        offset += 1;
        let length = usize::try_from(value.unsigned_abs()).expect("u32 fits usize");
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= face_list.len())
            .ok_or_else(|| {
                fail(
                    r,
                    "hsf.invalid_face_list",
                    Status::Invalid,
                    "Face extends beyond the face list",
                )
            })?;
        let ids = &face_list[offset..end];
        offset = end;
        if length < 3 || ids.iter().any(|i| *i < 0 || *i as usize >= vertices.len()) {
            return Err(fail(
                r,
                "hsf.invalid_face_list",
                Status::Invalid,
                "Face has fewer than three vertices or an out-of-range index",
            ));
        }
        if !strips && (value < 0 || length > 4 || (length == 4 && !convex_quad(&vertices, ids))) {
            unsupported = Some(
                "Holes, polygons with more than four vertices, and non-convex/non-planar/degenerate quads are retained without triangulation",
            );
        }
        if unsupported.is_none() {
            for i in 1..length - 1 {
                charge(count, 1, options.max_triangles, "hsf.limit_triangles")?;
                budget.charge_output(32)?;
                let tri = if strips && value > 0 {
                    if i % 2 == 1 {
                        [ids[i - 1], ids[i], ids[i + 1]]
                    } else {
                        [ids[i], ids[i - 1], ids[i + 1]]
                    }
                } else {
                    [ids[0], ids[i], ids[i + 1]]
                };
                triangles.push(tri.map(|v| v as u32));
                triangle_faces.push(face);
            }
        }
        face += 1;
    }
    if unsupported.is_some() {
        triangles.clear();
        triangle_faces.clear();
    }
    let bounds = vertices.first().map(|first| {
        let mut result = [*first, *first];
        for v in &vertices {
            for (i, value) in v.iter().enumerate() {
                result[0][i] = result[0][i].min(*value);
                result[1][i] = result[1][i].max(*value);
            }
        }
        result
    });
    let parameters = parameters(doc, r, budget)?;
    Ok((
        Mesh {
            owner_node_id: String::new(),
            vertex_normals,
            entity_metadata,
            refinement: None,
            vertex_parameters: parameters.0,
            parameters: parameters.1,
            id: String::new(),
            source: r.source.clone(),
            source_record_id: r.id.clone(),
            vertex_count: vertices.len() as u64,
            face_value_count: face_list.len() as u64,
            triangle_count: triangles.len() as u64,
            normal_count: normals.len() as u64,
            topology: if strips {
                "strips_and_fans"
            } else {
                "polygons"
            }
            .into(),
            triangulation: if unsupported.is_some() {
                Status::Partial
            } else {
                Status::Complete
            },
            bounds,
            vertices,
            face_list,
            triangles,
            triangle_faces,
            normals,
        },
        unsupported,
    ))
}

fn grid(
    doc: &HsfDocument,
    r: &Record,
    options: &SceneOptions,
    count: &mut u64,
    budget: &mut Budget,
) -> Result<Mesh, HsfError> {
    let l = r.mesh.as_ref().expect("framed rectangular mesh");
    let cells = u64::from(l.rows - 1) * u64::from(l.columns - 1);
    charge(
        count,
        cells * 2,
        options.max_triangles,
        "hsf.limit_triangles",
    )?;
    // Account for vertices, optional normals, all derived indices and capacity growth.
    budget.charge_output(2048 + u64::from(l.vertex_count) * 24 + cells * 128)?;
    let vertices = vectors(doc, &l.vertices, r)?;
    let (vertex_normals, normals) = polyhedron::normals(doc, r, budget)?;
    let mut triangles = Vec::new();
    let mut triangle_faces = Vec::new();
    let mut face_list = Vec::new();
    // Mesh row-major triangle numbering, with the diagonal from (r+1,c)
    // to (r,c+1). This is not the polygon fan diagonal used for Shell quads.
    for row in 0..l.rows - 1 {
        for col in 0..l.columns - 1 {
            let a = row * l.columns + col;
            let b = a + l.columns;
            for tri in [[a, b, a + 1], [b + 1, a + 1, b]] {
                triangle_faces.push(triangles.len() as u32);
                face_list.push(3);
                face_list.extend(tri.map(|i| i as i32));
                triangles.push(tri);
            }
        }
    }
    let mut bounds = [vertices[0], vertices[0]];
    for v in &vertices {
        for (i, x) in v.iter().enumerate() {
            bounds[0][i] = bounds[0][i].min(*x);
            bounds[1][i] = bounds[1][i].max(*x);
        }
    }
    let parameters = parameters(doc, r, budget)?;
    Ok(Mesh {
        owner_node_id: String::new(),
        vertex_normals,
        entity_metadata: None,
        refinement: None,
        vertex_parameters: parameters.0,
        parameters: parameters.1,
        id: String::new(),
        source: r.source.clone(),
        source_record_id: r.id.clone(),
        vertex_count: u64::from(l.vertex_count),
        face_value_count: face_list.len() as u64,
        triangle_count: triangles.len() as u64,
        normal_count: normals.len() as u64,
        topology: "rectangular_grid".into(),
        triangulation: Status::Complete,
        bounds: Some(bounds),
        vertices,
        face_list,
        triangles,
        triangle_faces,
        normals,
    })
}
