use hsf_core::*;

fn shell(refinement: bool) -> Vec<u8> {
    let mut b = vec![b'S', if refinement { 8 } else { 24 }];
    if refinement {
        b.extend(0u32.to_le_bytes());
    }
    b.push(0); // LOD
    b.extend(3u32.to_le_bytes());
    for v in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
        b.extend(v.to_le_bytes());
    }
    b.extend([1, 5, 0, 0, 0, 8, 3, 0, 1, 2]);
    b.extend([3, 1, 2]);
    for v in [-2f32, -3., 4., 5.] {
        b.extend(v.to_le_bytes());
    }
    b.extend([12, 12, 0, 0, 0]);
    // Independent bit-string fixture: six 12-bit codes, crossing 32-bit words.
    // 000 fff 555 7ff 001 ffe; last 24 bits are padding.
    b.extend([0x55, 0xff, 0x0f, 0, 0x1f, 0, 0xff, 0x57, 0, 0, 0, 0xfe]);
    b.push(0);
    b
}
fn read(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<Scene>, HsfError> {
    let input = [b";; HSF V19.10 \n".as_slice(), body, &[0]].concat();
    let mut budget = Budget::new(Limits::default());
    let parsed = parse(&input, &ParseOptions::default(), &mut budget)?;
    scene(
        &parsed,
        &SceneOptions {
            mode,
            ..Default::default()
        },
        &mut budget,
    )
}
#[test]
fn quantized_codes_cross_words_and_reconstruct_stored_bounds() {
    let s = read(&shell(false), ParseMode::Strict).unwrap();
    let m = &s.data.meshes[0];
    let q = m
        .vertex_parameters
        .as_ref()
        .unwrap()
        .quantization
        .as_ref()
        .unwrap();
    assert_eq!(q.packing, "le32_msb");
    assert_eq!(q.bounds, [[-2., 4.], [-3., 5.]]);
    let expected: Vec<f32> = [0u32, 4095, 1365, 2047, 1, 4094]
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let (lo, hi) = if i % 2 == 0 { (-2., 4.) } else { (-3., 5.) };
            (lo + (hi - lo) * (f64::from(v) / 4095.)) as f32
        })
        .collect();
    assert_eq!(m.parameters, expected);
    assert_eq!(s.data.instances.len(), 1);
}

#[test]
fn five_component_parameters_use_word_packing_and_retain_tuple_width() {
    let original = shell(false);
    // Replace the known final attribute, keeping the independently authored
    // three-vertex shell. Fifteen 8-bit values occupy four little-endian words.
    let prefix_length = 3 + 4 + 9 * 4 + 10; // opcode/flags/LOD, count, XYZ, faces
    let mut body = original[..prefix_length].to_vec();
    body.extend([3, 1, 5]);
    for value in [0f32, 0., 0., 0., 0., 255., 255., 255., 255., 255.] {
        body.extend(value.to_le_bytes());
    }
    body.push(8);
    body.extend(16i32.to_le_bytes());
    body.extend([3, 2, 1, 0, 7, 6, 5, 4, 11, 10, 9, 8, 255, 14, 13, 12]);
    body.push(0);
    let result = read(&body, ParseMode::Strict).unwrap();
    let mesh = &result.data.meshes[0];
    let attribute = mesh.vertex_parameters.as_ref().unwrap();
    assert_eq!((attribute.width, attribute.count), (5, 3));
    assert_eq!(attribute.values_source.length, 16);
    assert_eq!(attribute.quantization.as_ref().unwrap().packing, "le32_msb");
    assert_eq!(
        mesh.parameters,
        (0..15).map(|v| v as f32).collect::<Vec<_>>()
    );
    assert_eq!(mesh.triangles, [[0, 1, 2]]);
}
#[test]
fn refinement_geometry_preserves_null_identity_and_never_backfills_tags() {
    let body = [shell(true), b"S\x96\x02\0\0q".to_vec(), shell(true)].concat();
    let s = read(&body, ParseMode::Partial).unwrap();
    assert_eq!(s.data.null_shells.len(), 1);
    assert_eq!(s.data.meshes.len(), 1);
    assert!(s.data.instances.is_empty()); // unresolved earlier update withholds placements
    let a = &s.data.shell_parameter_sets;
    assert_eq!(a.len(), 2);
    assert_eq!(
        a[0].association_reason.as_deref(),
        Some("hsf.parameter_target_unresolved")
    );
    assert_eq!(a[1].association_status, Status::Complete);
    assert_eq!(
        a[1].target_id.as_deref(),
        Some(s.data.null_shells[0].id.as_str())
    );
    assert_eq!(a[1].parameters.len(), 6);
    assert_eq!(a[1].interpretation_status, Status::Complete);
    assert_eq!(a[1].mesh_id.as_deref(), Some(s.data.meshes[0].id.as_str()));
    assert_eq!(
        read(&body, ParseMode::Strict).unwrap_err().diagnostic.code,
        "hsf.shell_lod"
    );
}

#[test]
fn unique_refinement_uses_original_owner_and_reference_identity() {
    let body = [
        b"(\x01aS\x96\x02\0\0q)(\x01b".to_vec(),
        shell(true),
        b")r\0\0\0\0".to_vec(),
    ]
    .concat();
    let s = read(&body, ParseMode::Strict).unwrap();
    let m = &s.data.meshes[0];
    let r = m.refinement.as_ref().unwrap();
    assert_eq!(r.target_node_id, s.data.nodes[1].id);
    assert_eq!(m.owner_node_id, r.target_node_id);
    assert_ne!(m.owner_node_id, r.context_node_id);
    assert_eq!(r.context_node_id, s.data.nodes[2].id);
    assert_eq!(r.application_status, Status::Complete);
    assert_eq!(
        s.data.null_shells[0].mesh_id.as_deref(),
        Some(m.id.as_str())
    );
    assert_eq!(
        s.data.references[0].target_id.as_deref(),
        Some(s.data.null_shells[0].id.as_str())
    );
    assert_eq!(s.data.instances.len(), 2);
    assert!(
        s.data
            .instances
            .iter()
            .all(|i| i.node_id != s.data.nodes[2].id)
    );
    assert_eq!(m.triangles, [[0, 1, 2]]);
    assert_eq!(s.data.shell_parameter_sets[0].parameters, m.parameters);
}

#[test]
fn duplicate_refinement_revokes_geometry_and_parameter_application() {
    let body = [
        b"S\x96\x02\0\0q".to_vec(),
        shell(true),
        shell(true),
        shell(true),
    ]
    .concat();
    let s = read(&body, ParseMode::Partial).unwrap();
    assert_eq!(s.data.meshes.len(), 1);
    assert!(s.data.null_shells[0].mesh_id.is_none());
    assert!(s.data.instances.is_empty());
    assert_eq!(
        s.data.shell_parameter_sets[0].interpretation_status,
        Status::Unsupported
    );
    assert_eq!(
        s.data.meshes[0]
            .refinement
            .as_ref()
            .unwrap()
            .application_reason
            .as_deref(),
        Some("hsf.refinement_duplicate")
    );
    assert_eq!(
        read(&body, ParseMode::Strict).unwrap_err().diagnostic.code,
        "hsf.refinement_duplicate"
    );
}
