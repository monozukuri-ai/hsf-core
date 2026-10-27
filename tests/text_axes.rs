use hsf_core::*;

fn wire(body: &[u8]) -> Vec<u8> {
    [b";; HSF V19.10 \n".as_slice(), body].concat()
}
fn parse_body(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        &wire(body),
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
fn batch() -> Vec<u8> {
    // X is shared globally, Y is free, Z is shared within each component.
    let mut b = vec![16];
    b.extend(0x3202u16.to_le_bytes());
    for n in [5i32, 2, 2, 3] {
        b.extend(n.to_le_bytes());
    }
    for v in [0f32, 1., 2., 3., 4., 12., 34., 56.] {
        b.extend(v.to_le_bytes());
    }
    b
}
fn region() -> Vec<u8> {
    let mut b = vec![b'x'];
    for v in [4f32, 5., 6.] {
        b.extend(v.to_le_bytes());
    }
    b.extend([5, 2, b'A', 0, 1, 31, 2]);
    for v in [0f32, 1., 2., 3., 4., 5.] {
        b.extend(v.to_le_bytes());
    }
    b
}
#[test]
fn shared_coordinates_and_component_reference_identity() {
    let p = parse_body(
        &[batch(), b"qr\0\0\0\0\0".to_vec()].concat(),
        ParseMode::Strict,
    )
    .unwrap();
    let l = p.data.records[0].poly_polyline.as_ref().unwrap();
    assert_eq!((l.dimensions, l.primitive_count, l.point_count), (1, 2, 5));
    assert_eq!(l.leftovers.length, 12);
    assert_eq!(l.leftovers.offset, l.points.offset + l.points.length);
    let s = scene(
        &p,
        &SceneOptions {
            mode: ParseMode::Strict,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    let poly = &s.data.primitives[0];
    assert_eq!(
        poly.points,
        [
            [12., 0., 34.],
            [12., 1., 34.],
            [12., 2., 56.],
            [12., 3., 56.],
            [12., 4., 56.]
        ]
    );
    assert_eq!(poly.part_lengths, [2, 3]);
    assert_eq!(s.data.primitive_instances.len(), 2);
    assert_eq!(s.data.references[0].target_id.as_ref(), Some(&poly.id));
}
#[test]
fn text_region_is_raw_metadata_with_unevaluated_scene_effects() {
    let p = parse_body(&[region(), batch(), vec![0]].concat(), ParseMode::Strict).unwrap();
    let l = p.data.records[0].primitive.as_ref().unwrap();
    let r = l.text_region.as_ref().unwrap();
    assert_eq!((l.point_count, r.point_count, r.options), (1, 2, 31));
    assert_eq!(p.data.source_bytes(&r.points), Some(&region()[20..]));
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert_eq!(s.data.primitives.len(), 2);
    assert_eq!(
        s.data.primitives[0].text.as_ref().unwrap().value.as_deref(),
        Some("A")
    );
    assert_eq!(s.data.primitives[0].points, [[4., 5., 6.]]);
    assert_eq!(s.data.placement_status, Status::Unsupported);
    assert!(s.data.primitive_instances.is_empty());
    assert_eq!(s.data.rendering_options.len(), 1);
    let e = scene(
        &p,
        &SceneOptions {
            mode: ParseMode::Strict,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.text_region_semantics");
}
#[test]
fn truncation_nonfinite_and_unknown_fields_fail_closed() {
    for record in [batch(), region()] {
        for cut in 0..record.len() {
            for mode in [ParseMode::Strict, ParseMode::Partial] {
                assert_eq!(
                    parse_body(&record[..cut], mode)
                        .unwrap_err()
                        .diagnostic
                        .classification,
                    Status::Invalid
                );
            }
        }
        let mut bad = record;
        let at = bad.len() - 4;
        bad[at..].copy_from_slice(&f32::NAN.to_le_bytes());
        bad.push(0);
        assert_eq!(
            parse_body(&bad, ParseMode::Partial)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::Invalid
        );
    }
    for (at, value, code) in [
        (17, 2, "hsf.text_options"),
        (18, 32, "hsf.text_region_options"),
        (19, 5, "hsf.text_region_count"),
    ] {
        let mut b = region();
        b[at] = value;
        b.push(0);
        let p = parse_body(&b, ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty());
        assert_eq!(p.data.domains[0].parsed_bytes, p.data.header.byte_len);
        assert_eq!(p.diagnostics[0].code, code);
    }
}
#[test]
fn region_vertices_and_components_share_existing_limits() {
    for (b, options, code) in [
        (
            [region(), vec![0]].concat(),
            ParseOptions {
                max_vertices: 2,
                ..Default::default()
            },
            "hsf.limit_vertices",
        ),
        (
            [batch(), vec![0]].concat(),
            ParseOptions {
                max_records: 1,
                ..Default::default()
            },
            "hsf.limit_poly_polyline_parts",
        ),
    ] {
        assert_eq!(
            parse(&wire(&b), &options, &mut Budget::new(Limits::default()))
                .unwrap_err()
                .diagnostic
                .code,
            code
        );
    }
    let p = parse_body(&[batch(), vec![0]].concat(), ParseMode::Strict).unwrap();
    assert_eq!(
        scene(
            &p,
            &SceneOptions {
                max_primitives: 1,
                ..Default::default()
            },
            &mut Budget::new(Limits::default())
        )
        .unwrap_err()
        .diagnostic
        .code,
        "hsf.limit_primitives"
    );
}
