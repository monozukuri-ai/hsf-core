use hsf_core::*;

fn parsed(
    body: &[u8],
    version: &str,
    mode: ParseMode,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        &[format!(";; HSF V{version} \n").as_bytes(), body].concat(),
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn batch(flags: u16) -> Vec<u8> {
    let mut b = vec![16];
    b.extend(flags.to_le_bytes());
    b.extend(5i32.to_le_bytes());
    if flags & 2 != 0 {
        b.extend(2i32.to_le_bytes());
    }
    b.extend(2i32.to_le_bytes());
    b.extend(3i32.to_le_bytes());
    for i in 0..5 {
        for axis in 0..3 {
            if flags & (0x100 << (axis * 2)) == 0 {
                b.extend(((i * 3 + axis) as f32).to_le_bytes());
            }
        }
    }
    b
}
fn shell(refinement: bool) -> Vec<u8> {
    let mut b = vec![b'S', if refinement { 10 } else { 26 }];
    if refinement {
        b.extend(123u32.to_le_bytes());
    }
    b.push(0);
    b.extend(3u32.to_le_bytes());
    b.extend(floats(&[0., 0., 0., 1., 0., 0., 0., 1., 0.]));
    b.extend([1, 5, 0, 0, 0, 8, 3, 0, 1, 2]);
    b.extend([1, 7, 12, 12, 0, 0, 0]);
    b.extend([0; 12]);
    b.extend([3, 1, 1]);
    b.extend(floats(&[0., 1.]));
    b.extend([8, 4, 0, 0, 0, 10, 20, 30, 0]);
    b.extend(b"\x63U\x03\0x=y\x16\x01\0A\0\0\0");
    b
}
fn rendering() -> Vec<u8> {
    let mut b = vec![b'R'];
    for n in [0x81000000u32, 0x110, 0x01000000, 0x110] {
        b.extend(n.to_le_bytes());
    }
    b.extend([0xa0, 0x80, 0x10, 0, 0]); // HLR flags and HSR algorithm
    b.extend(floats(&[0., 0.1]));
    b.push(2);
    b
}
fn scene_of(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<Scene>, HsfError> {
    scene(
        &parsed(body, "19.10", ParseMode::Strict)?,
        &SceneOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
#[test]
fn markers_and_batches_preserve_zero_axes_lengths_and_reference_identity() {
    for flags in [0, 2, 0x100, 0x400, 0x1000, 0x1402] {
        let b = [
            b"X".to_vec(),
            floats(&[1., 2., 3.]),
            batch(flags),
            b"qr\0\0\0\0\0".to_vec(),
        ]
        .concat();
        let s = scene_of(&b, ParseMode::Strict).unwrap();
        assert_eq!(s.status, Status::Complete);
        assert_eq!(s.data.primitives[0].kind, "marker");
        let p = &s.data.primitives[1];
        assert_eq!(p.part_lengths, [2, 3]);
        assert_eq!(p.kind, "poly_polyline");
        for (i, point) in p.points.iter().enumerate() {
            for (axis, v) in point.iter().enumerate() {
                assert_eq!(
                    *v,
                    if flags & (0x100 << (axis * 2)) != 0 {
                        0.
                    } else {
                        (i * 3 + axis) as f32
                    }
                );
            }
        }
        assert_eq!(s.data.primitive_instances.len(), 3);
        assert_eq!(
            s.data.references[0].target_id.as_deref(),
            Some("primitive:1")
        );
    }
}
#[test]
fn shell_refinements_keep_metadata_and_never_become_new_meshes() {
    let b = [shell(true), vec![0]].concat();
    let p = parsed(&b, "19.10", ParseMode::Strict).unwrap();
    let l = p.data.records[0].shell.as_ref().unwrap();
    assert_eq!(l.refinement_index, Some(123));
    assert_eq!(l.attributes.len(), 3);
    assert!(l.normals.is_none());
    assert_eq!(
        p.data
            .source_bytes(&l.attributes[2].fields[0].source)
            .unwrap(),
        b"x=y"
    );
    let s = scene_of(&b, ParseMode::Partial).unwrap();
    assert!(s.data.meshes.is_empty());
    assert_eq!(s.diagnostics[0].code, "hsf.shell_lod");
    assert_eq!(
        scene_of(&b, ParseMode::Strict).unwrap_err().diagnostic.code,
        "hsf.shell_lod"
    );
    let b = [shell(false), vec![0]].concat();
    let s = scene_of(&b, ParseMode::Partial).unwrap();
    assert_eq!(s.data.meshes[0].triangles, [[0, 1, 2]]);
    assert_eq!(s.data.meshes[0].normals.len(), 3);
    assert_eq!(s.data.meshes[0].normals[0][1], 1.);
    let meta = s.data.meshes[0].entity_metadata.as_ref().unwrap();
    assert_eq!(meta.fields.len(), 2);
    assert_eq!(meta.fields[0].text.value.as_deref(), Some("x=y"));
    assert_eq!(meta.fields[1].text.value.as_deref(), Some("A"));
    assert_eq!(meta.decoding_status, Status::Complete);
    assert_eq!(meta.interpretation_status, Status::Unsupported);
    assert_eq!(s.data.instances.len(), 1);
    assert_eq!(s.data.placement_status, Status::Complete);
    assert_eq!(
        scene_of(&b, ParseMode::Strict).unwrap().status,
        Status::Complete
    );
}
#[test]
fn render_nested_fields_and_texture_matrix_are_retained_without_effects() {
    let matrix = [
        vec![b'$'],
        floats(&[1., 0., 0., 0., 1., 0., 0., 0., 1., 2., 3., 4.]),
    ]
    .concat();
    let b = [rendering(), matrix, vec![0]].concat();
    let p = parsed(&b, "14.50", ParseMode::Strict).unwrap();
    let f = &p.data.records[0].rendering.as_ref().unwrap().fields;
    assert_eq!(
        f.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        [
            "hidden_line_options",
            "hidden_line_hsr",
            "depth_range",
            "simple_shadow_options"
        ]
    );
    let s = scene_of(&b, ParseMode::Partial).unwrap();
    assert_eq!(s.data.rendering_options.len(), 2);
    assert!(s.data.nodes[0].local_transform.is_none());
    assert_eq!(s.data.placement_status, Status::Unsupported);
}
#[test]
fn extended_color_selectors_are_opaque_and_version_bounded() {
    let b = b"\"\x80\x81\x80\x02\x01\x05white\0";
    let p = parsed(b, "19.10", ParseMode::Strict).unwrap();
    assert_eq!(
        p.data.records[0].rendering.as_ref().unwrap().mask,
        0x02808180
    );
    assert_eq!(
        scene_of(b, ParseMode::Strict).unwrap_err().diagnostic.code,
        "hsf.rendering_semantics"
    );
    assert_eq!(
        parsed(b, "14.50", ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.color_mask"
    );
    assert_eq!(
        parsed(
            b"~\x80\x81\x80\x02\xff\xff\xff\0",
            "19.10",
            ParseMode::Strict
        )
        .unwrap_err()
        .diagnostic
        .code,
        "hsf.color_mask"
    );
}
#[test]
fn all_operand_cuts_are_invalid_in_both_modes() {
    for b in [
        shell(true),
        batch(0x1000),
        batch(2),
        rendering(),
        [vec![b'$'], floats(&[1.; 12])].concat(),
        [vec![b'X'], floats(&[1., 2., 3.])].concat(),
    ] {
        for end in 0..b.len() {
            for mode in [ParseMode::Strict, ParseMode::Partial] {
                assert_eq!(
                    parsed(&b[..end], "19.10", mode)
                        .unwrap_err()
                        .diagnostic
                        .classification,
                    Status::Invalid,
                    "cut {end} op {}",
                    b[0]
                );
            }
        }
    }
}
#[test]
fn bad_lengths_numbers_unknown_flags_and_limits_fail_closed() {
    for flags in [1u16, 4, 0x4000, 0x8000, 0x1500] {
        let b = [vec![16], flags.to_le_bytes().to_vec(), b"h\x01\0".to_vec()].concat();
        let p = parsed(&b, "19.10", ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty());
        assert_eq!(p.data.domains[0].parsed_bytes, p.data.header.byte_len);
    }
    for n in [0i32, -1, 6] {
        let mut b = batch(0);
        b[7..11].copy_from_slice(&n.to_le_bytes());
        b.push(0);
        assert_eq!(
            parsed(&b, "19.10", ParseMode::Partial)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::Invalid
        );
    }
    let mut b = batch(0);
    b[15..19].copy_from_slice(&f32::NAN.to_le_bytes());
    b.push(0);
    assert_eq!(
        parsed(&b, "19.10", ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.nonfinite_poly_polyline"
    );
    let p = parsed(
        &[batch(0), batch(0), vec![0]].concat(),
        "19.10",
        ParseMode::Strict,
    )
    .unwrap();
    let e = scene(
        &p,
        &SceneOptions {
            max_primitives: 3,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_primitives");
    let mut b = shell(true);
    b[60..64].copy_from_slice(&8u32.to_le_bytes());
    b.push(0);
    assert_eq!(
        parsed(&b, "19.10", ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .classification,
        Status::Invalid
    );
}
