use hsf_core::*;

fn wire(body: &[u8], version: &str) -> Vec<u8> {
    [format!(";; HSF V{version} \n").as_bytes(), body].concat()
}
fn parsed(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        &wire(body, "19.10"),
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
fn polygon() -> Vec<u8> {
    let mut b = vec![b'G'];
    b.extend(5i32.to_le_bytes());
    // Concave, with no repeated closing point. No triangle fan is inferred.
    for value in [0f32, 0., 0., 2., 0., 0., 1., 1., 0., 2., 2., 0., 0., 2., 0.] {
        b.extend(value.to_le_bytes());
    }
    b
}
fn palette() -> Vec<u8> {
    b"\x0c\0\x02\0\0\0\xff\0\0\0\xff\0".to_vec()
}
#[test]
fn polygon_order_closure_identity_and_versions() {
    for version in ["8.05", "14.50", "19.10"] {
        let b = [polygon(), b"qr\0\0\0\0\0".to_vec()].concat();
        let p = parse(
            &wire(&b, version),
            &ParseOptions::default(),
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        let s = scene(
            &p,
            &SceneOptions {
                mode: ParseMode::Strict,
                ..Default::default()
            },
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        assert_eq!(s.status, Status::Complete);
        assert_eq!(s.data.primitives.len(), 1);
        let poly = &s.data.primitives[0];
        assert_eq!(poly.kind, "polygon");
        assert_eq!(poly.point_count, 5);
        assert_eq!(
            poly.points,
            [
                [0., 0., 0.],
                [2., 0., 0.],
                [1., 1., 0.],
                [2., 2., 0.],
                [0., 2., 0.]
            ]
        );
        assert!(poly.part_lengths.is_empty());
        assert_eq!(s.data.primitive_instances.len(), 2);
        assert_eq!(s.data.references[0].target_id.as_ref(), Some(&poly.id));
        assert!(s.data.meshes.is_empty());
        assert_eq!(
            p.data.source_bytes(&poly.points_source),
            Some(&polygon()[5..])
        );
    }
}
#[test]
fn palette_framing_is_distinct_from_scene_interpretation() {
    let b = [palette(), polygon(), vec![0]].concat();
    let p = parsed(&b, ParseMode::Strict).unwrap();
    let map = p.data.records[0].color_map.as_ref().unwrap();
    assert_eq!((map.format, map.entry_count), (0, 2));
    assert_eq!(p.data.source_bytes(&map.colors), Some(&palette()[6..]));
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert_eq!(s.data.rendering_options[0].kind, "color_map");
    assert_eq!(s.data.primitives.len(), 1);
    assert_eq!(s.data.placement_status, Status::Unsupported);
    assert!(s.data.primitive_instances.is_empty());
    let e = scene(
        &p,
        &SceneOptions {
            mode: ParseMode::Strict,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.rendering_semantics");
}
#[test]
fn cuts_and_invalid_numbers_never_turn_into_partial_success() {
    for b in [polygon(), palette()] {
        for cut in 0..b.len() {
            for mode in [ParseMode::Strict, ParseMode::Partial] {
                assert_eq!(
                    parsed(&b[..cut], mode)
                        .unwrap_err()
                        .diagnostic
                        .classification,
                    Status::Invalid
                );
            }
        }
    }
    for n in [-1i32, 0, 1, 2] {
        let b = [vec![b'G'], n.to_le_bytes().to_vec(), vec![0]].concat();
        assert_eq!(
            parsed(&b, ParseMode::Partial)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::Invalid
        );
    }
    for f in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut b = polygon();
        b[5..9].copy_from_slice(&f.to_le_bytes());
        b.push(0);
        assert_eq!(
            parsed(&b, ParseMode::Strict).unwrap_err().diagnostic.code,
            "hsf.nonfinite_polygon"
        );
    }
    assert_eq!(
        parsed(b"\x0c\0\xff\xff\xff\xff\0", ParseMode::Partial)
            .unwrap_err()
            .diagnostic
            .classification,
        Status::Invalid
    );
}
#[test]
fn unknown_map_format_stops_at_its_opcode_without_resynchronizing() {
    for format in [1, 2, 255] {
        let b = [vec![12, format], polygon(), vec![0]].concat();
        let p = parsed(&b, ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty());
        assert_eq!(p.data.domains[0].parsed_bytes, p.data.header.byte_len);
        assert_eq!(p.diagnostics[0].code, "hsf.color_map_format");
        assert_eq!(p.diagnostics[0].opcode, Some(12));
        assert_eq!(
            parsed(&b, ParseMode::Strict)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::Unsupported
        );
    }
}
#[test]
fn counts_and_payloads_are_bounded_before_allocation() {
    let b = wire(&[polygon(), polygon(), vec![0]].concat(), "19.10");
    let e = parse(
        &b,
        &ParseOptions {
            max_vertices: 9,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_vertices");
    let p = parsed(&[polygon(), polygon(), vec![0]].concat(), ParseMode::Strict).unwrap();
    let e = scene(
        &p,
        &SceneOptions {
            max_primitives: 1,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_primitives");
    let e = parse(
        &wire(&[palette(), vec![0]].concat(), "19.10"),
        &ParseOptions {
            max_string_bytes: 5,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_string");
    assert_eq!(
        parsed(b"\x0c\0\x01\0\x01\0", ParseMode::Partial)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.limit_color_map"
    );
}
