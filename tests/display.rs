use hsf_core::*;

fn parse_body(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        &[b";; HSF V19.10 \n".as_slice(), body].concat(),
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
fn scene_body(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<Scene>, HsfError> {
    let p = parse_body(body, mode)?;
    scene(
        &p,
        &SceneOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
fn visibility(mask: u32, value: u32) -> Vec<u8> {
    let mut b = vec![b'V', mask as u8, value as u8];
    if mask & 128 != 0 {
        b.extend(((mask >> 8) as u16).to_le_bytes());
        b.extend(((value >> 8) as u16).to_le_bytes());
    }
    if mask & 0x800000 != 0 {
        b.extend([(mask >> 24) as u8, (value >> 24) as u8]);
    }
    b
}
fn size(value: f32, units: Option<u8>) -> Vec<u8> {
    let mut b = vec![b'+'];
    b.extend(value.to_le_bytes());
    b.extend(units);
    b
}
fn font(data: &[u8]) -> Vec<u8> {
    let mut b = b"f\0\x01F\x05\x02fn".to_vec();
    b.extend((data.len() as i32).to_le_bytes());
    b.extend(data);
    b
}
fn line() -> Vec<u8> {
    [vec![b'l'], vec![0; 24]].concat()
}

#[test]
fn exact_framing_and_masks_in_all_supported_versions() {
    for version in ["8.05", "14.50", "19.10"] {
        for (mask, value) in [(1, 0), (0xffff, 0xaa55), (0x03ffffff, 0x01010001)] {
            let mut b = visibility(mask, value);
            b.push(0);
            let p = parse(
                &[format!(";; HSF V{version} \n").as_bytes(), &b].concat(),
                &ParseOptions::default(),
                &mut Budget::new(Limits::default()),
            )
            .unwrap();
            if version == "8.05" && mask & 0x800000 != 0 {
                assert!(p.data.records.is_empty());
                assert_eq!(p.diagnostics[0].code, "hsf.visibility_extension_version");
                continue;
            }
            let r = &p.data.records[0];
            assert_eq!(r.visibility, Some(VisibilityLayout { mask, value }));
            assert_eq!(r.source.length, b.len() as u64 - 1);
            let s = scene(
                &p,
                &SceneOptions::default(),
                &mut Budget::new(Limits::default()),
            )
            .unwrap();
            for bit in 0..32 {
                let a = &s.data.nodes[0].display.visibility[bit];
                if mask & 0x037fff7f & (1 << bit) == 0 {
                    assert!(a.is_none());
                } else {
                    assert_eq!(a.as_ref().unwrap().value, value & (1 << bit) != 0);
                }
            }
        }
    }
}

#[test]
fn observed_visibility_bit26_has_fixed_framing_but_no_placement_semantics() {
    for value in [0, 0x0400_0001] {
        let record = visibility(0x0480_0081, value);
        let body = [record.clone(), line(), vec![0]].concat();
        let p = parse_body(&body, ParseMode::Strict).unwrap();
        assert_eq!(p.status, Status::Complete);
        assert_eq!(p.data.records[0].source.length, 9);
        assert_eq!(p.data.records[1].source.offset, 24);
        assert_eq!(p.data.records[0].visibility.as_ref().unwrap().value, value);
        let s = scene_body(&body, ParseMode::Partial).unwrap();
        assert_eq!(s.status, Status::Partial);
        assert_eq!(s.data.primitives.len(), 1);
        assert!(s.data.primitive_instances.is_empty());
        assert_eq!(s.data.placement_status, Status::Unsupported);
        assert_eq!(s.diagnostics[0].code, "hsf.visibility_geometry_bit26");
        assert!(s.data.nodes[0].display.visibility[26].is_none());
        assert_eq!(
            s.data.nodes[0].display.visibility[0]
                .as_ref()
                .unwrap()
                .value,
            value & 1 != 0
        );
        assert_eq!(
            scene_body(&body, ParseMode::Strict)
                .unwrap_err()
                .diagnostic
                .code,
            "hsf.visibility_geometry_bit26"
        );
        for cut in 1..record.len() {
            assert_eq!(
                parse_body(&record[..cut], ParseMode::Partial)
                    .unwrap_err()
                    .diagnostic
                    .classification,
                Status::Invalid
            );
        }
        let old = [b";; HSF V14.50 \n".as_slice(), &body].concat();
        let p = parse(
            &old,
            &ParseOptions::default(),
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        assert_eq!(p.diagnostics[0].code, "hsf.visibility_mask");
        assert!(p.data.records.is_empty());
    }
}

#[test]
fn every_cut_is_invalid_and_unknown_fields_stop_without_resync() {
    let records = [
        visibility(0x03ffffff, 1),
        b"J\x01a\x03\0h\0z".to_vec(),
        font(b"]h\0\xff"),
        size(-2., Some(4)),
        size(0.5, None),
    ];
    for record in &records {
        for cut in 0..record.len() {
            for mode in [ParseMode::Partial, ParseMode::Strict] {
                assert_eq!(
                    parse_body(&record[..cut], mode)
                        .unwrap_err()
                        .diagnostic
                        .classification,
                    Status::Invalid
                );
            }
        }
    }
    for (record, code) in [
        (visibility(0x08800080, 0), "hsf.visibility_mask"),
        (size(-1., Some(5)), "hsf.marker_size_units"),
        (b"f\x01h\0".to_vec(), "hsf.font_format"),
        (b"~\x80\x80\x80\x80".to_vec(), "hsf.color_mask"),
    ] {
        let b = [b"h\0".as_slice(), &record, b"h\x01\0"].concat();
        let p = parse_body(&b, ParseMode::Partial).unwrap();
        assert_eq!(p.data.records.len(), 1);
        assert_eq!(p.unparsed[0].offset, 17);
        assert_eq!(p.diagnostics[0].code, code);
        assert_eq!(
            parse_body(&b, ParseMode::Strict)
                .unwrap_err()
                .diagnostic
                .code,
            code
        );
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            parse_body(&size(value, Some(1)), ParseMode::Partial)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::Invalid
        );
    }
    let mut b = font(b"");
    let n = b.len();
    b[n - 4..].copy_from_slice(&(-1i32).to_le_bytes());
    assert_eq!(
        parse_body(&b, ParseMode::Partial)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.negative_count"
    );
}

#[test]
fn marker_units_visibility_inherit_override_and_reopen_without_changing_geometry() {
    let mut b = visibility(0x03ffffff, 0x01000201);
    b.extend(size(0.5, None));
    b.extend(b"(\x03defq");
    b.extend(line());
    b.extend(b")");
    b.extend(b"(\x01a");
    b.extend(visibility(0x00000280, 0));
    b.extend(size(-2., Some(4)));
    b.extend(b"<\x04/def)(\x01b<\x04/def)\0");
    let s = scene_body(&b, ParseMode::Strict).unwrap();
    assert_eq!(s.data.primitive_instances.len(), 3);
    for (i, expected) in s.data.primitive_instances.iter().zip([
        (0.5, "relative", true),
        (2., "pixels", false),
        (0.5, "relative", true),
    ]) {
        let a = i.display.marker_size.as_ref().unwrap();
        assert_eq!(
            (a.value.value, a.value.units.as_str()),
            (expected.0, expected.1)
        );
        assert_eq!(i.display.visibility[9].as_ref().unwrap().value, expected.2);
        assert!(i.display.visibility[24].as_ref().unwrap().value);
        assert_eq!(i.world_transform, IDENTITY);
    }
    b.pop();
    b.extend(b"s\0\0\0\0");
    b.extend(size(-3., Some(3)));
    b.extend(b")\0");
    let s = scene_body(&b, ParseMode::Strict).unwrap();
    assert!(
        s.data.primitive_instances.iter().all(|i| i
            .display
            .marker_size
            .as_ref()
            .unwrap()
            .value
            .units
            == "points")
    );
    for (code, units) in [
        (0, "object"),
        (1, "screen"),
        (2, "window"),
        (3, "points"),
        (4, "pixels"),
    ] {
        let s = scene_body(
            &[size(-1., Some(code)), vec![0]].concat(),
            ParseMode::Strict,
        )
        .unwrap();
        assert_eq!(
            s.data.nodes[0]
                .display
                .marker_size
                .as_ref()
                .unwrap()
                .value
                .units,
            units
        );
    }
    for value in [0., -0.] {
        let p = parse_body(&[size(value, None), vec![0]].concat(), ParseMode::Strict).unwrap();
        assert_eq!(
            p.data.records[0].marker_size.as_ref().unwrap().value_bits,
            value.to_bits()
        );
    }
}

#[test]
fn definitions_keep_duplicates_raw_data_and_distinct_name_encoding() {
    let mut b = b"J\x01a\x05\0solidJ\x01a\x03\0\xff\0z".to_vec();
    let data = vec![0xff; 300];
    b.extend(font(&data));
    b.extend(line());
    b.push(0);
    let p = parse_body(&b, ParseMode::Strict).unwrap();
    assert_eq!(p.data.records[2].font.as_ref().unwrap().data.length, 300);
    let s = scene_body(&b, ParseMode::Partial).unwrap();
    assert_eq!(s.data.line_styles.len(), 2);
    assert_eq!(s.data.fonts.len(), 1);
    assert_eq!(
        s.data.line_styles[0].definition.value.as_deref(),
        Some("solid")
    );
    assert_eq!(s.data.line_styles[1].definition.value, None);
    assert_eq!(s.data.fonts[0].name.value.as_deref(), Some("F"));
    assert_eq!(s.data.fonts[0].name.encoding, "ascii");
    assert_eq!(s.data.fonts[0].encoding, 5);
    assert_eq!(
        p.data.source_bytes(&s.data.fonts[0].data_source).unwrap(),
        data
    );
    assert_eq!(s.status, Status::Partial);
    assert!(s.data.primitive_instances.is_empty());
    assert_eq!(s.data.primitives.len(), 1);
    assert_eq!(
        scene_body(&b, ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.line_style_definition"
    );
    let e = scene(
        &p,
        &SceneOptions {
            max_definitions: 2,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_definitions");
    let e = parse(
        &[b";; HSF V19.10 \n".as_slice(), &font(&data), &[0]].concat(),
        &ParseOptions {
            max_string_bytes: 299,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.classification, Status::LimitExceeded);
}

#[test]
fn line_style_version_gate_and_budget_fail_before_interpretation() {
    let body = b"J\0\0\0\0";
    let p = parse(
        &[b";; HSF V8.05 \n".as_slice(), body].concat(),
        &ParseOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert!(p.data.records.is_empty());
    assert_eq!(p.diagnostics[0].code, "hsf.line_style_version");
    let p = parse_body(
        &[visibility(0x03ffffff, 1), line(), vec![0]].concat(),
        ParseMode::Strict,
    )
    .unwrap();
    let e = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits {
            max_output_bytes: 8191,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.classification, Status::LimitExceeded);
}
