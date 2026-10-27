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
fn floats(a: &[f32]) -> Vec<u8> {
    a.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn camera() -> Vec<u8> {
    [
        b">\0".to_vec(),
        floats(&[0., 0., 10., 0., 0., 0., 0., 1., 0., 20., 10.]),
    ]
    .concat()
}
fn texture() -> Vec<u8> {
    let mut b = b"t\x03tex\x03img".to_vec();
    b.extend(0x7ffffu32.to_le_bytes());
    b.extend([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    b.extend(floats(&[2., 0.5]));
    b.extend(b"\x03../");
    b.extend([3, 2]);
    b
}
fn grid(with_params: bool) -> Vec<u8> {
    let mut b = vec![b'M', if with_params { 8 } else { 0 }];
    b.extend(3u32.to_le_bytes());
    b.extend(2u32.to_le_bytes());
    for y in 0..3 {
        for x in 0..2 {
            b.extend(floats(&[x as f32, y as f32, 0.]));
        }
    }
    if with_params {
        b.extend([3, 1, 3]);
        b.extend(floats(&[0., 0., 0., 1., 1., 1.]));
        b.push(8);
        b.extend(18u32.to_le_bytes());
        b.extend([0; 18]);
        b.push(0);
    }
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
fn pause_preserves_scope_tag_target_and_requires_a_real_terminator() {
    let b = [
        b"(\x01a\x01".to_vec(),
        grid(false),
        vec![1],
        camera(),
        b"qr\0\0\0\0)\0".to_vec(),
    ]
    .concat();
    let p = parsed(&b, "19.10", ParseMode::Strict).unwrap();
    let pauses: Vec<_> = p
        .data
        .records
        .iter()
        .filter(|r| r.kind == "pause")
        .collect();
    assert_eq!(pauses.len(), 2);
    assert!(
        pauses
            .iter()
            .all(|r| r.source.length == 1 && r.scope.as_deref() == Some("record:0"))
    );
    let s = scene_of(&b, ParseMode::Strict).unwrap();
    assert_eq!(s.status, Status::Complete);
    assert_eq!(s.data.instances.len(), 2);
    assert_eq!(s.data.references[0].target_id.as_deref(), Some("mesh:0"));
    assert_eq!(s.data.cameras[0].node_id, "segment:1");
    assert!(s.data.views.is_empty());
    for mode in [ParseMode::Strict, ParseMode::Partial] {
        assert_eq!(
            parsed(&[1], "19.10", mode).unwrap_err().diagnostic.code,
            "hsf.missing_terminator"
        );
    }
}
#[test]
fn rectangular_mesh_has_native_diagonal_and_triangle_face_numbers() {
    let b = [grid(false), vec![0]].concat();
    let s = scene_of(&b, ParseMode::Strict).unwrap();
    let m = &s.data.meshes[0];
    assert_eq!(m.vertices.len(), 6);
    assert_eq!(m.topology, "rectangular_grid");
    assert_eq!(m.triangles, [[0, 2, 1], [3, 1, 2], [2, 4, 3], [5, 3, 4]]);
    assert_eq!(m.triangle_faces, [0, 1, 2, 3]);
    assert_eq!(
        m.face_list,
        [3, 0, 2, 1, 3, 3, 1, 2, 3, 2, 4, 3, 3, 5, 3, 4]
    );
    assert_eq!(m.bounds, Some([[0., 0., 0.], [1., 2., 0.]]));
    let p = parsed(&[grid(true), vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
    let a = &p.data.records[0].mesh.as_ref().unwrap().attributes[0];
    assert_eq!((a.opcode, a.width, a.bits_per_sample), (3, 3, Some(8)));
    assert_eq!(p.data.source_bytes(&a.values).unwrap(), [0; 18]);
    let s = scene_of(&[grid(true), vec![0]].concat(), ParseMode::Partial).unwrap();
    assert_eq!(s.data.meshes[0].triangles, m.triangles);
    assert_eq!(s.data.instances.len(), 1);
    assert_eq!(s.data.placement_status, Status::Complete);
    let strict = scene_of(&[grid(true), vec![0]].concat(), ParseMode::Strict).unwrap();
    assert_eq!(strict.data.meshes[0].parameters, vec![0.; 18]);
    assert_eq!(
        strict.data.meshes[0]
            .vertex_parameters
            .as_ref()
            .unwrap()
            .quantization
            .as_ref()
            .unwrap()
            .packing,
        "u8_xyz"
    );
}
#[test]
fn normal_and_uncompressed_parameter_attributes_preserve_values() {
    let mut b = grid(false);
    b[1] = 8;
    b.push(17);
    b.extend(floats(&[0., 0., 1.].repeat(6)));
    b.extend([27, 2]);
    b.extend(floats(&[0., 1.].repeat(6)));
    b.extend([0, 0]);
    let p = parsed(&b, "19.10", ParseMode::Strict).unwrap();
    let l = p.data.records[0].mesh.as_ref().unwrap();
    assert_eq!(l.attributes.len(), 2);
    let s = scene_of(&b, ParseMode::Partial).unwrap();
    assert_eq!(s.data.meshes[0].normals, [[0., 0., 1.]; 6]);
    assert_eq!(l.attributes[1].values.length, 48);
    // Older compressed parameters have an implicit width of three.
    let mut b = grid(true);
    b.remove(84);
    assert_eq!(
        parsed(&[b, vec![0]].concat(), "8.05", ParseMode::Strict)
            .unwrap()
            .data
            .records[0]
            .mesh
            .as_ref()
            .unwrap()
            .attributes[0]
            .width,
        3
    );
}
#[test]
fn texture_options_strings_and_conditions_are_source_backed() {
    let b = [
        texture(),
        b"?\xfe\x04\0flag\x08\x01\x07\x09\x02\x01\x02\0".to_vec(),
    ]
    .concat();
    let p = parsed(&b, "19.10", ParseMode::Strict).unwrap();
    let t = p.data.records[0].texture.as_ref().unwrap();
    assert_eq!(t.flags, 0x7ffff);
    assert_eq!(p.data.source_bytes(&t.name).unwrap(), b"tex");
    assert_eq!(p.data.source_bytes(&t.image).unwrap(), b"img");
    assert_eq!(t.fields.len(), 14);
    assert_eq!(t.fields[10].source.length, 8);
    assert_eq!(t.fields[11].name, "transform_segment");
    assert_eq!(p.data.records[3].indexed_color.as_ref().unwrap().index, 513);
    let s = scene_of(&b, ParseMode::Partial).unwrap();
    assert_eq!(s.data.textures.len(), 1);
    assert_eq!(s.data.rendering_options.len(), 3);
    assert_eq!(s.data.placement_status, Status::Unsupported);
    let b = b"~\x01\x01\x02\x03\x08\x01\x07\0";
    let s = scene_of(b, ParseMode::Partial).unwrap();
    assert!(s.data.nodes[0].display.colors[0].is_none());
    assert!(s.data.nodes[0].face_color.is_none());
    // Long texture name uses a signed i32 escape, independently of image length.
    let b = [
        b"t\xff".to_vec(),
        256u32.to_le_bytes().to_vec(),
        vec![b'a'; 256],
        b"\0\0\0\0".to_vec(),
    ]
    .concat();
    assert_eq!(
        parsed(&b, "19.10", ParseMode::Strict).unwrap().data.records[0]
            .texture
            .as_ref()
            .unwrap()
            .name
            .length,
        256
    );
}
#[test]
fn every_known_operand_truncation_fails_in_both_modes() {
    for b in [
        camera(),
        texture(),
        grid(false),
        grid(true),
        b"?\xff\x04\0\0\0flag".to_vec(),
        b"\x09\x81\x80\x80\x01\xff\x7f".to_vec(),
    ] {
        for end in 0..b.len() {
            for mode in [ParseMode::Partial, ParseMode::Strict] {
                assert_eq!(
                    parsed(&b[..end], "19.10", mode)
                        .unwrap_err()
                        .diagnostic
                        .classification,
                    Status::Invalid,
                    "end {end} opcode {}",
                    b[0]
                );
            }
        }
    }
}
#[test]
fn unknown_profiles_stop_at_opcode_without_scanning() {
    let mut bad_attribute = grid(false);
    bad_attribute[1] = 8;
    bad_attribute.push(99);
    let mut bad_bits = grid(true);
    bad_bits[109] = 7;
    for (body, version, code) in [
        (texture(), "8.05", "hsf.texture_version"),
        (b"t\0\0\0\x80\0\x80".to_vec(), "19.10", "hsf.texture_flags"),
        (b"?\0".to_vec(), "8.05", "hsf.conditions_version"),
        (
            b">\x80".to_vec(),
            "19.10",
            "hsf.unsupported_camera_projection",
        ),
        (
            b">\x10".to_vec(),
            "8.05",
            "hsf.unsupported_camera_near_limit_version",
        ),
        (b"M\x01".to_vec(), "19.10", "hsf.mesh_flags"),
        (bad_attribute, "19.10", "hsf.mesh_attribute"),
        (bad_bits, "19.10", "hsf.mesh_parameter_bits"),
    ] {
        let body = [body, b"h\x01\0".to_vec()].concat();
        let p = parsed(&body, version, ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty());
        assert_eq!(p.diagnostics[0].code, code);
        assert_eq!(p.data.domains[0].parsed_bytes, p.data.header.byte_len);
        assert_eq!(
            parsed(&body, version, ParseMode::Strict)
                .unwrap_err()
                .diagnostic
                .code,
            code
        );
    }
}
#[test]
fn invalid_values_counts_and_resource_budgets_remain_fatal() {
    let mut bad_length = grid(true);
    bad_length[110..114].copy_from_slice(&17u32.to_le_bytes());
    let mut bad_float = grid(false);
    bad_float[10..14].copy_from_slice(&f32::NAN.to_le_bytes());
    for b in [
        bad_length,
        bad_float,
        b"?\xff\xff\xff\xff\xff".to_vec(),
        [
            b"M\0".to_vec(),
            [i32::MAX, i32::MAX]
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect(),
        ]
        .concat(),
    ] {
        for mode in [ParseMode::Strict, ParseMode::Partial] {
            assert_eq!(
                parsed(&b, "19.10", mode)
                    .unwrap_err()
                    .diagnostic
                    .classification,
                Status::Invalid
            );
        }
    }
    let b = [b";; HSF V19.10 \n".to_vec(), grid(false), vec![0]].concat();
    for options in [
        ParseOptions {
            max_vertices: 5,
            ..Default::default()
        },
        ParseOptions {
            max_face_values: 15,
            ..Default::default()
        },
    ] {
        assert_eq!(
            parse(&b, &options, &mut Budget::new(Limits::default()))
                .unwrap_err()
                .diagnostic
                .classification,
            Status::LimitExceeded
        );
    }
    let p = parsed(&[grid(false), vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
    assert_eq!(
        scene(
            &p,
            &SceneOptions {
                max_triangles: 3,
                ..Default::default()
            },
            &mut Budget::new(Limits::default())
        )
        .unwrap_err()
        .diagnostic
        .code,
        "hsf.limit_triangles"
    );
    let p = parsed(&[camera(), vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
    assert_eq!(
        scene(
            &p,
            &SceneOptions {
                max_views: 0,
                ..Default::default()
            },
            &mut Budget::new(Limits::default())
        )
        .unwrap_err()
        .diagnostic
        .code,
        "hsf.limit_views"
    );
    let mut b = camera();
    b[2..6].copy_from_slice(&f32::NAN.to_le_bytes());
    b.push(0);
    assert_eq!(
        scene_of(&b, ParseMode::Partial)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.nonfinite_camera"
    );
    let p = parsed(&[texture(), vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
    assert_eq!(
        scene(
            &p,
            &SceneOptions {
                max_definitions: 0,
                ..Default::default()
            },
            &mut Budget::new(Limits::default())
        )
        .unwrap_err()
        .diagnostic
        .code,
        "hsf.limit_definitions"
    );
}
