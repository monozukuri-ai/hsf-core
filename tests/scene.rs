use hsf_core::*;
fn shell(points: &[[f32; 3]], faces: &[i32], bits: u8, flags: u8, signed: bool) -> Vec<u8> {
    let mut b = vec![b'S', flags | 16 | if signed { 128 } else { 0 }];
    if signed {
        b.extend(4u16.to_le_bytes());
    }
    b.push(0);
    b.extend((points.len() as i32).to_le_bytes());
    for p in points {
        for v in p {
            b.extend(v.to_le_bytes());
        }
    }
    b.push(1);
    b.extend((1 + faces.len() as i32 * i32::from(bits / 8)).to_le_bytes());
    b.push(bits);
    for f in faces {
        b.extend(&f.to_le_bytes()[..usize::from(bits / 8)]);
    }
    if flags & 8 != 0 {
        b.push(0x11);
        for _ in points {
            for v in [0f32, 0., 1.] {
                b.extend(v.to_le_bytes());
            }
        }
        b.push(0);
    }
    b
}
const QUAD: [[f32; 3]; 4] = [[0., 0., 0.], [2., 0., 0.], [2., 3., 0.], [0., 3., 0.]];
fn file(body: &[u8]) -> Vec<u8> {
    [b";; HSF V19.10 \n".as_slice(), body, b"\0"].concat()
}
fn read(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<Scene>, HsfError> {
    let mut budget = Budget::new(Limits::default());
    let p = parse(
        &file(body),
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
fn transform(values: [f32; 12]) -> Vec<u8> {
    let mut b = vec![b'%'];
    for v in values {
        b.extend(v.to_le_bytes());
    }
    b
}
#[test]
fn widths_normals_quad_and_triangle_provenance() {
    for bits in [8, 16, 32] {
        for signed in [false, true] {
            let data = shell(&QUAD, &[4, 0, 1, 2, 3], bits, 8, signed);
            let result = read(&data, ParseMode::Strict).unwrap();
            assert_eq!(result.status, Status::Complete);
            assert_eq!(result.data.units, None);
            let m = &result.data.meshes[0];
            assert_eq!(m.owner_node_id, result.data.nodes[0].id);
            assert_eq!(m.vertices, QUAD);
            assert_eq!(m.face_list, [4, 0, 1, 2, 3]);
            assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3]]);
            assert_eq!(m.triangle_faces, [0, 0]);
            assert_eq!(m.normals, [[0., 0., 1.]; 4]);
            assert_eq!(m.bounds, Some([[0., 0., 0.], [2., 3., 0.]]));
            assert_eq!(result.data.instances.len(), 1);
            assert_eq!(result.data.instances[0].world_transform, IDENTITY);
            assert!(result.data.instances[0].face_color.is_none());
            assert!(result.data.instances[0].face_visible.is_none());
        }
    }
}
#[test]
fn strip_winding_fan_and_holes() {
    let p = read(
        &shell(&QUAD, &[4, 0, 1, 2, 3, -4, 0, 1, 2, 3], 8, 4, true),
        ParseMode::Strict,
    )
    .unwrap();
    assert_eq!(
        p.data.meshes[0].triangles,
        [[0, 1, 2], [2, 1, 3], [0, 1, 2], [0, 2, 3]]
    );
    assert_eq!(p.data.meshes[0].triangle_faces, [0, 0, 1, 1]);
    let body = shell(&QUAD, &[4, 0, 1, 2, 3, -3, 0, 1, 2], 16, 0, true);
    let p = read(&body, ParseMode::Partial).unwrap();
    assert_eq!(p.status, Status::Partial);
    assert_eq!(p.data.meshes[0].face_list.len(), 9);
    assert!(p.data.meshes[0].triangles.is_empty());
    assert_eq!(
        read(&body, ParseMode::Strict).unwrap_err().diagnostic.code,
        "hsf.shell_triangulation"
    );
    for points in [
        [[0., 0., 0.], [2., 0., 0.], [0.5, 0.5, 0.], [0., 2., 0.]],
        [[0., 0., 0.], [2., 0., 0.], [2., 2., 0.], [0., 2., 1.]],
    ] {
        assert_eq!(
            read(
                &shell(&points, &[4, 0, 1, 2, 3], 8, 0, false),
                ParseMode::Partial
            )
            .unwrap()
            .status,
            Status::Partial
        );
    }
}
#[test]
fn references_keep_definitions_and_compose_row_vector_matrices() {
    let rotation = transform([0., 1., 0., -1., 0., 0., 0., 0., 1., 10., 20., 30.]);
    let mirror = transform([-1., 0., 0., 0., 1., 0., 0., 0., 1., 2., 0., 0.]);
    let mut body = b"(\x03def~\x01\xff\0\0".to_vec();
    body.extend(shell(&QUAD, &[3, 0, 1, 2], 8, 0, false));
    body.extend(b"q)q(\x05place");
    body.extend(rotation);
    body.extend(b"~\x01\0\0\xffV\x01\0(\x05child");
    body.extend(mirror);
    body.extend(b"r\0\0\0\0))");
    let p = read(&body, ParseMode::Strict).unwrap();
    assert_eq!(p.data.meshes.len(), 1);
    assert_eq!(p.data.meshes[0].owner_node_id, p.data.nodes[1].id);
    assert_eq!(p.data.instances.len(), 2);
    let i = &p.data.instances[1];
    assert_eq!(i.determinant, -1.);
    assert_eq!(&i.world_transform[12..15], &[10., 22., 30.]);
    // (2,0,0) mirrored to (0,0,0), then parent translation.
    assert_eq!(
        [
            2. * i.world_transform[0] + i.world_transform[12],
            2. * i.world_transform[1] + i.world_transform[13]
        ],
        [10., 20.]
    );
    assert_eq!(i.face_color.as_ref().unwrap().value, [0, 0, 255]);
    assert!(!i.face_visible.as_ref().unwrap().value);
    assert_eq!(
        p.data.instances[0].face_color.as_ref().unwrap().value,
        [255, 0, 0]
    );
    // Reopening applies final attributes even to earlier geometry; include uses
    // the referenced segment's local attributes, not its original ancestors.
    body.extend(b"s\x01\0\0\0~\x01\0\xff\0)(\x01x<\x04/def)");
    let p = read(&body, ParseMode::Strict).unwrap();
    assert_eq!(p.data.instances.len(), 3);
    assert_eq!(
        p.data.instances[0].face_color.as_ref().unwrap().value,
        [0, 255, 0]
    );
    assert_eq!(
        p.data.instances[2].face_color.as_ref().unwrap().value,
        [0, 255, 0]
    );
}
#[test]
fn repeat_new_geometry_and_repeated_tag() {
    let mut body = shell(&QUAD, &[3, 0, 1, 2], 8, 0, false);
    body.extend(b"q&");
    for n in [0i32, 0, -1, 0] {
        body.extend(n.to_le_bytes());
    }
    body.push(0);
    body.extend(&transform([1., 0., 0., 0., 1., 0., 0., 0., 1., 7., 8., 9.])[1..]);
    body.extend(b"qr\x01\0\0\0");
    let p = read(&body, ParseMode::Strict).unwrap();
    assert_eq!(p.data.meshes.len(), 1);
    assert_eq!(p.data.instances.len(), 3);
    for i in &p.data.instances[1..] {
        assert_eq!(&i.world_transform[12..15], &[7., 8., 9.]);
    }
}
#[test]
fn cycles_unresolved_conditionals_and_style_are_explicit() {
    for body in [
        b"(\x01aqr\0\0\0\0)".as_slice(),
        b"r\xff\0\0\0",
        b"<\0\x01a\x01x",
        b"{\x01a",
    ] {
        let p = read(body, ParseMode::Partial).unwrap();
        assert_eq!(p.status, Status::Partial);
        assert!(!p.diagnostics.is_empty());
        assert!(p.data.instances.is_empty());
        assert_eq!(
            read(body, ParseMode::Strict)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::Unsupported
        );
    }
}
#[test]
fn every_cut_invalid_and_geometry_validation_never_partial_success() {
    let data = file(&shell(&QUAD, &[4, 0, 1, 2, 3], 32, 8, true));
    for cut in 0..data.len() {
        let e = parse(
            &data[..cut],
            &ParseOptions::default(),
            &mut Budget::new(Limits::default()),
        )
        .unwrap_err();
        assert_eq!(e.diagnostic.classification, Status::Invalid, "cut={cut}");
    }
    for body in [
        shell(&QUAD, &[3, 0, 1, 9], 8, 0, false),
        shell(&QUAD, &[4, 0, 1], 8, 0, false),
        shell(&[[f32::NAN, 0., 0.]; 4], &[3, 0, 1, 2], 8, 0, false),
        transform([f32::INFINITY; 12]),
    ] {
        for mode in [ParseMode::Strict, ParseMode::Partial] {
            assert_eq!(
                read(&body, mode).unwrap_err().diagnostic.classification,
                Status::Invalid
            );
        }
    }
}
#[test]
fn unsupported_shell_profiles_stop_at_the_shell_without_scanning() {
    for (prefix, code) in [
        (vec![b'S', 0x50], "hsf.shell_connectivity_compression"),
        (vec![b'S', 0x11], "hsf.shell_quantized_points"),
        (vec![b'S', 0x90, 1, 0], "hsf.shell_flags"),
        (vec![b'~', 0x80, 0x80, 0x80, 0x80], "hsf.color_mask"),
    ] {
        let body = [prefix.as_slice(), b"(\x01x)"].concat();
        let p = read(&body, ParseMode::Partial).unwrap();
        assert_eq!(p.status, Status::Partial);
        assert!(p.data.meshes.is_empty());
        assert_eq!(p.diagnostics[0].code, code);
        assert_eq!(p.unparsed[0].offset, 15);
        assert_eq!(
            read(&body, ParseMode::Strict).unwrap_err().diagnostic.code,
            code
        );
    }
    let mut lod = shell(&QUAD, &[3, 0, 1, 2], 8, 0, false);
    lod[2] = 1;
    assert_eq!(
        read(&lod, ParseMode::Partial).unwrap().diagnostics[0].code,
        "hsf.shell_lod"
    );
}
#[test]
fn budgets_cover_syntax_scene_and_expansion_work() {
    let data = file(&shell(&QUAD, &[4, 0, 1, 2, 3], 8, 0, false));
    for options in [
        ParseOptions {
            max_vertices: 3,
            ..Default::default()
        },
        ParseOptions {
            max_face_values: 4,
            ..Default::default()
        },
    ] {
        assert_eq!(
            parse(&data, &options, &mut Budget::new(Limits::default()))
                .unwrap_err()
                .diagnostic
                .classification,
            Status::LimitExceeded
        );
    }
    for opts in [
        SceneOptions {
            max_meshes: 0,
            ..Default::default()
        },
        SceneOptions {
            max_nodes: 0,
            ..Default::default()
        },
        SceneOptions {
            max_instances: 0,
            ..Default::default()
        },
        SceneOptions {
            max_depth: 0,
            ..Default::default()
        },
        SceneOptions {
            max_visits: 0,
            ..Default::default()
        },
        SceneOptions {
            max_triangles: 1,
            ..Default::default()
        },
    ] {
        let mut budget = Budget::new(Limits::default());
        let p = parse(&data, &ParseOptions::default(), &mut budget).unwrap();
        assert_eq!(
            scene(&p, &opts, &mut budget)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::LimitExceeded
        );
    }
    let mut budget = Budget::new(Limits::default());
    let p = parse(&data, &ParseOptions::default(), &mut budget).unwrap();
    let before = budget.usage().output_bytes;
    let mut small = Budget::new(Limits {
        max_output_bytes: before + 1000,
        ..Default::default()
    });
    small.charge_output(before).unwrap();
    assert_eq!(
        scene(&p, &SceneOptions::default(), &mut small)
            .unwrap_err()
            .diagnostic
            .code,
        "budget.output"
    );
}

#[test]
fn names_and_empty_scene_have_explicit_boundaries() {
    assert_eq!(
        read(b"", ParseMode::Strict).unwrap().status,
        Status::Complete
    );
    let partial = read(b"[\0", ParseMode::Partial).unwrap();
    assert_eq!(partial.status, Status::Partial);
    assert!(partial.data.meshes.is_empty());
    for name in [b"\xff".as_slice(), b"\xc3\xa9"] {
        let body = [b"(".as_slice(), &[name.len() as u8], name, b")"].concat();
        let p = read(&body, ParseMode::Partial).unwrap();
        assert_eq!(p.diagnostics[0].code, "hsf.segment_name_encoding");
        assert_eq!(p.data.processed_records, 0);
        assert_eq!(p.unparsed.len(), 0); // syntax is framed; semantic scope stopped
    }
}

#[test]
fn acyclic_reference_expansion_is_bounded_even_without_geometry() {
    let mut body = b"(\x01a)q".to_vec();
    for i in 0..12u32 {
        body.extend([b'(', 1, b'b' + i as u8]);
        for _ in 0..2 {
            body.push(b'r');
            body.extend(i.to_le_bytes());
        }
        body.extend(b")q");
    }
    for opts in [
        SceneOptions {
            max_visits: 100,
            ..Default::default()
        },
        SceneOptions {
            max_depth: 5,
            ..Default::default()
        },
    ] {
        let mut budget = Budget::new(Limits::default());
        let p = parse(&file(&body), &ParseOptions::default(), &mut budget).unwrap();
        assert_eq!(
            scene(&p, &opts, &mut budget)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::LimitExceeded
        );
    }
}
