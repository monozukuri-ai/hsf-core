use hsf_core::*;

fn view(flags: u8, name: &[u8]) -> Vec<u8> {
    let mut b = vec![b'}', flags];
    for f in [0f32, 0., 10., 0., 0., 0., 0., 1., 0., 20., 30.] {
        b.extend(f.to_le_bytes());
    }
    for (bit, value) in [(4, 12f32), (8, -7.), (16, 0.25)] {
        if flags & bit != 0 {
            b.extend(value.to_le_bytes());
        }
    }
    b.push(name.len() as u8);
    b.extend(name);
    b
}
fn file(version: &str, body: &[u8]) -> Vec<u8> {
    [format!(";; HSF V{version} \n").as_bytes(), body].concat()
}
fn parse_body(
    version: &str,
    body: &[u8],
    mode: ParseMode,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        &file(version, body),
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
fn scene_body(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<Scene>, HsfError> {
    let mut budget = Budget::new(Limits::default());
    let p = parse(
        &file("19.10", body),
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut budget,
    )?;
    scene(
        &p,
        &SceneOptions {
            mode,
            ..Default::default()
        },
        &mut budget,
    )
}
fn line() -> Vec<u8> {
    let mut b = vec![b'l'];
    for f in [1f32, 2., 3., 4., 5., 6.] {
        b.extend(f.to_le_bytes());
    }
    b
}
#[test]
fn all_projection_masks_and_versions_keep_exact_name_and_float_spans() {
    for version in ["8.05", "14.50", "19.10"] {
        for flags in 0..32 {
            let mut b = view(flags, b"default");
            b.push(0);
            if version == "8.05" && flags & 16 != 0 {
                assert_eq!(
                    parse_body(version, &b, ParseMode::Strict)
                        .unwrap_err()
                        .diagnostic
                        .code,
                    "hsf.unsupported_view_near_limit_version"
                );
                continue;
            }
            let p = parse_body(version, &b, ParseMode::Strict).unwrap();
            let layout = p.data.records[0].view.as_ref().unwrap();
            assert_eq!(p.data.source_bytes(&layout.name).unwrap(), b"default");
            assert_eq!(
                layout.camera.length,
                44 + (flags & 28).count_ones() as u64 * 4
            );
            assert_eq!(p.data.records[0].source.length, b.len() as u64 - 1);
            let s = scene(
                &p,
                &SceneOptions {
                    mode: ParseMode::Strict,
                    ..Default::default()
                },
                &mut Budget::new(Limits::default()),
            )
            .unwrap();
            let v = &s.data.views[0];
            assert_eq!(s.status, Status::Complete);
            assert_eq!(v.name.value.as_deref(), Some("default"));
            assert_eq!(v.camera.position, [0., 0., 10.]);
            assert_eq!(v.camera.target, [0., 0., 0.]);
            assert_eq!(v.camera.up_vector, [0., 1., 0.]);
            assert_eq!(v.camera.field_width, 20.);
            assert_eq!(v.camera.field_height, 30.);
            assert_eq!(v.camera.stretched, flags & 2 != 0);
            assert_eq!(
                v.camera.projection,
                if flags & 1 != 0 {
                    "perspective"
                } else {
                    "orthographic"
                }
            );
            assert_eq!(v.camera.oblique_y_degrees, (flags & 4 != 0).then_some(12.));
            assert_eq!(v.camera.oblique_x_degrees, (flags & 8 != 0).then_some(-7.));
            assert_eq!(v.camera.near_limit, (flags & 16 != 0).then_some(0.25));
            assert_eq!(s.data.nodes.len(), 1);
            assert!(s.data.nodes[0].local_transform.is_none());
        }
    }
}
#[test]
fn cuts_unknown_flags_unknown_handedness_and_limits_never_scan() {
    for record in [view(31, b"]h\0"), b"h\x00".to_vec(), b"h\x01".to_vec()] {
        for cut in 0..record.len() {
            for mode in [ParseMode::Partial, ParseMode::Strict] {
                let e = parse_body("19.10", &record[..cut], mode).unwrap_err();
                assert_eq!(e.diagnostic.classification, Status::Invalid);
            }
        }
    }
    for (b, code) in [
        (b"h\x02h\x01\0".as_slice(), "hsf.unsupported_handedness"),
        (b"}\x20h\x01\0", "hsf.unsupported_view_projection"),
    ] {
        let p = parse_body("19.10", b, ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty());
        assert_eq!(p.diagnostics[0].code, code);
        assert_eq!(
            parse_body("19.10", b, ParseMode::Strict)
                .unwrap_err()
                .diagnostic
                .code,
            code
        );
    }
    let mut b = view(0, b"default");
    b.push(0);
    let e = parse(
        &file("19.10", &b),
        &ParseOptions {
            max_string_bytes: 6,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.classification, Status::LimitExceeded);
    let p = parse_body("19.10", &b, ParseMode::Strict).unwrap();
    let e = scene(
        &p,
        &SceneOptions {
            max_views: 0,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_views");
}
#[test]
fn view_validation_preserves_unknown_name_and_degenerate_basis() {
    for (offset, value, code) in [
        (2, f32::NAN, "hsf.nonfinite_view"),
        (38, 0., "hsf.invalid_view_field"),
        (46, f32::INFINITY, "hsf.nonfinite_view"),
    ] {
        let mut b = view(4, b"a");
        b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        b.push(0);
        for mode in [ParseMode::Partial, ParseMode::Strict] {
            assert_eq!(scene_body(&b, mode).unwrap_err().diagnostic.code, code);
        }
    }
    let mut b = view(0, b"\xff");
    b.push(0);
    let s = scene_body(&b, ParseMode::Partial).unwrap();
    assert_eq!(s.data.views[0].name.value, None);
    assert_eq!(s.data.views[0].status, Status::Unsupported);
    assert_eq!(
        scene_body(&b, ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.view_name_encoding"
    );
    let mut b = view(0, b"same");
    b[10..14].copy_from_slice(&0f32.to_le_bytes());
    b.push(0);
    let s = scene_body(&b, ParseMode::Partial).unwrap();
    assert_eq!(s.data.views[0].status, Status::Unsupported);
    assert_eq!(s.diagnostics[0].code, "hsf.degenerate_view");
}
#[test]
fn views_are_ordered_presets_not_transforms_or_sheets() {
    let mut b = b"(\x01a".to_vec();
    b.extend(view(1, b"default"));
    b.extend(line());
    b.extend(view(0, b"default"));
    b.extend(b")\0");
    let s = scene_body(&b, ParseMode::Strict).unwrap();
    assert_eq!(s.data.views.len(), 2);
    assert_ne!(s.data.views[0].id, s.data.views[1].id);
    assert_eq!(s.data.views[0].context_node_id, s.data.nodes[1].id);
    assert_eq!(s.data.primitive_instances[0].world_transform, IDENTITY);
    assert_eq!(s.data.primitives[0].points, [[1., 2., 3.], [4., 5., 6.]]);
    assert_eq!(s.data.placement_status, Status::Complete);
    // Tagging a view must not accidentally tag the preceding geometry.
    let mut b = line();
    b.extend(view(0, b"v"));
    b.extend(b"q\0");
    assert_eq!(
        scene_body(&b, ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.unknown_tag_target"
    );
}
#[test]
fn handedness_inherits_per_instance_and_preserves_coordinates() {
    let mut b = b"h\x01(\x03defq".to_vec();
    b.extend(line());
    b.extend(b")");
    b.extend(b"(\x01ah\x00<\x04/def)(\x01b<\x04/def)\0");
    let s = scene_body(&b, ParseMode::Strict).unwrap();
    assert_eq!(s.data.primitive_instances.len(), 3);
    let values: Vec<_> = s
        .data
        .primitive_instances
        .iter()
        .map(|i| i.world_handedness.as_ref().unwrap().value)
        .collect();
    assert_eq!(
        values,
        [Handedness::Right, Handedness::Left, Handedness::Right]
    );
    assert_eq!(s.data.primitives[0].points[0], [1., 2., 3.]);
    assert!(
        s.data
            .primitive_instances
            .iter()
            .all(|i| i.world_transform == IDENTITY)
    );
    // Reopening the definition sets a final stored attribute, overriding parents.
    b.pop();
    b.extend(b"s\0\0\0\0h\x00)\0");
    let s = scene_body(&b, ParseMode::Strict).unwrap();
    assert!(
        s.data
            .primitive_instances
            .iter()
            .all(|i| i.world_handedness.as_ref().unwrap().value == Handedness::Left)
    );
    let s = scene_body(&[line(), vec![0]].concat(), ParseMode::Strict).unwrap();
    assert!(s.data.primitive_instances[0].world_handedness.is_none());
}

#[test]
fn world_handedness_does_not_rewrite_mesh_winding_or_normals() {
    let mut b = b"h\x00S\x10\0".to_vec();
    b.extend(3i32.to_le_bytes());
    for f in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
        b.extend(f.to_le_bytes());
    }
    b.extend(b"\x01\x05\0\0\0\x08\x03\0\x01\x02\0");
    let s = scene_body(&b, ParseMode::Strict).unwrap();
    assert_eq!(
        s.data.meshes[0].vertices,
        [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
    );
    assert_eq!(s.data.meshes[0].triangles, [[0, 1, 2]]);
    assert!(s.data.meshes[0].normals.is_empty());
    assert_eq!(
        s.data.instances[0].world_handedness.as_ref().unwrap().value,
        Handedness::Left
    );
    assert_eq!(s.data.instances[0].world_transform, IDENTITY);
}
