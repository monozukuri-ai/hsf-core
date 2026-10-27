use hsf_core::*;

fn points(values: &[[f32; 3]]) -> Vec<u8> {
    values
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}
fn read(version: &str, body: &[u8], mode: ParseMode) -> Result<ParseOutcome<Scene>, HsfError> {
    let data = [format!(";; HSF V{version} \n").as_bytes(), body, b"\0"].concat();
    let mut budget = Budget::new(Limits::default());
    let parsed = parse(
        &data,
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut budget,
    )?;
    scene(
        &parsed,
        &SceneOptions {
            mode,
            ..Default::default()
        },
        &mut budget,
    )
}
fn text(code: u8, bytes: &[u8], options: bool) -> Vec<u8> {
    let mut b = [
        b"x".as_slice(),
        &points(&[[2., 3., 4.]]),
        &[code, 255],
        &(bytes.len() as u32).to_le_bytes(),
        bytes,
    ]
    .concat();
    if options {
        b.push(0);
    }
    b
}

#[test]
fn curve_control_points_and_versioned_center_are_source_faithful() {
    let p = [[5., 0., 7.], [0., 5., 7.], [-5., 0., 7.]];
    for version in ["8.05", "14.50", "19.10"] {
        let modern = version != "8.05";
        let mut b = vec![b'l'];
        b.extend(points(&p[..2]));
        b.push(b'L');
        b.extend(3i32.to_le_bytes());
        b.extend(points(&p));
        for op in [b'C', b'c'] {
            b.push(op);
            b.extend(points(&p));
            if modern {
                b.push(1);
                b.extend(points(&[[0., 0., 7.]]));
            }
        }
        let result = read(version, &b, ParseMode::Strict).unwrap();
        assert_eq!(result.status, Status::Complete);
        assert_eq!(result.data.primitives.len(), 4);
        assert_eq!(result.data.primitive_instances.len(), 4);
        assert_eq!(result.data.primitives[0].points, p[..2]);
        for item in &result.data.primitives[2..] {
            assert_eq!(item.points, p);
            assert_eq!(
                item.center.as_ref().map(|a| a.value),
                modern.then_some([0., 0., 7.])
            );
        }
    }
}

#[test]
fn japanese_and_supplementary_characters_are_lossless_and_raw_is_retained() {
    let expected = "寸法 10 mm 🦀";
    let u16: Vec<u8> = expected
        .encode_utf16()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let u32: Vec<u8> = expected
        .chars()
        .flat_map(|v| (v as u32).to_le_bytes())
        .collect();
    for (encoding, bytes) in [(5, u16), (6, u32), (10, expected.as_bytes().to_vec())] {
        for version in ["8.05", "19.10"] {
            let result = read(
                version,
                &text(encoding, &bytes, version != "8.05"),
                ParseMode::Strict,
            )
            .unwrap();
            let t = result.data.primitives[0].text.as_ref().unwrap();
            assert_eq!(t.value.as_deref(), Some(expected));
            assert_eq!(t.status, Status::Complete);
            assert_eq!(t.source.length, bytes.len() as u64);
        }
    }
    let result = read("19.10", &text(0, b"caf\xe9", true), ParseMode::Strict).unwrap();
    assert_eq!(
        result.data.primitives[0]
            .text
            .as_ref()
            .unwrap()
            .value
            .as_deref(),
        Some("café")
    );
}

#[test]
fn unknown_text_encoding_preserves_placement_but_invalid_unicode_errors() {
    let b = text(2, b"\x82\xa0", true);
    let result = read("19.10", &b, ParseMode::Partial).unwrap();
    assert_eq!(result.status, Status::Partial);
    assert!(
        result.data.primitives[0]
            .text
            .as_ref()
            .unwrap()
            .value
            .is_none()
    );
    assert_eq!(result.data.primitive_instances.len(), 1);
    assert_eq!(result.diagnostics[0].code, "hsf.text_encoding");
    assert_eq!(
        read("19.10", &b, ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.text_encoding"
    );
    for (code, bytes) in [
        (5, b"\0\xd8".as_slice()),
        (5, b"x"),
        (6, b"\0\0\x11\0"),
        (6, b"\0\xd8\0\0"),
        (10, b"\xc0\x80"),         // overlong NUL
        (10, b"\xe0\x80\xaf"),     // overlong 3-byte sequence
        (10, b"\xed\xa0\x80"),     // UTF-16 surrogate
        (10, b"\xf4\x90\x80\x80"), // above Unicode maximum
        (10, b"\x80"),             // standalone continuation
        (10, b"\xe5\xaf"),         // truncated character
        (10, b"\xe5A\xb8"),        // invalid continuation
        (10, b"\xff"),
    ] {
        for mode in [ParseMode::Strict, ParseMode::Partial] {
            assert_eq!(
                read("19.10", &text(code, bytes, true), mode)
                    .unwrap_err()
                    .diagnostic
                    .code,
                "hsf.invalid_text_encoding"
            );
        }
    }
}

#[test]
fn utf8_preserves_scalar_boundaries_nul_bom_and_combining_sequences() {
    // No trimming, BOM removal, normalization, or replacement is performed.
    let expected =
        "\0\u{7f}\u{80}\u{7ff}\u{800}\u{d7ff}\u{e000}\u{ffff}\u{10000}\u{10ffff}\u{feff}e\u{301}";
    for value in ["", expected] {
        let result = read(
            "19.10",
            &text(10, value.as_bytes(), true),
            ParseMode::Strict,
        )
        .unwrap();
        let t = result.data.primitives[0].text.as_ref().unwrap();
        assert_eq!(t.encoding, "utf_8");
        assert_eq!(t.encoding_code, Some(10));
        assert_eq!(t.value.as_deref(), Some(value));
        assert_eq!(t.source.length, value.len() as u64);
        assert_eq!(result.data.primitive_instances.len(), 1);
    }
}

#[test]
fn segment_options_preserve_node_identity_without_product_interpretation() {
    let value = "refname=部品,configname=標準";
    let bytes: Vec<u8> = value.encode_utf16().flat_map(|v| v.to_le_bytes()).collect();
    let b = [
        b"(\x01aU\x03\0x=y\x16\xff\xff".as_slice(),
        &((bytes.len() / 2) as u32).to_le_bytes(),
        &bytes,
        b")",
    ]
    .concat();
    let result = read("19.10", &b, ParseMode::Strict).unwrap();
    assert_eq!(result.data.user_options.len(), 2);
    assert_eq!(
        result.data.user_options[1].text.value.as_deref(),
        Some(value)
    );
    assert_eq!(result.data.user_options[1].node_id, result.data.nodes[1].id);
    assert_eq!(
        result.data.user_options[0].text.value.as_deref(),
        Some("x=y")
    );
}

#[test]
fn references_repeats_and_per_mask_attributes_apply_to_primitives() {
    let mut b = b"(\x01a~\x14\x01\x02\x03V\x14\x04l".to_vec();
    b.extend(points(&[[0., 0., 0.], [1., 0., 0.]]));
    b.extend(b"q)q(\x01b%");
    b.extend(
        [1f32, 0., 0., 0., 1., 0., 0., 0., 1., 10., 20., 30.]
            .iter()
            .flat_map(|v| v.to_le_bytes()),
    );
    b.extend(b"<\x02/a&");
    for v in [0i32, 0, -1, 0] {
        b.extend(v.to_le_bytes());
    }
    b.push(0);
    b.extend(
        [1f32, 0., 0., 0., 1., 0., 0., 0., 1., 2., 0., 0.]
            .iter()
            .flat_map(|v| v.to_le_bytes()),
    );
    b.extend(b")s\x01\0\0\0~\x04\x09\x08\x07)");
    let result = read("19.10", &b, ParseMode::Strict).unwrap();
    assert_eq!(result.data.primitives.len(), 1);
    assert_eq!(
        result.data.primitives[0].owner_node_id,
        result.data.nodes[1].id
    );
    let instances = &result.data.primitive_instances;
    assert_eq!(instances.len(), 3);
    for i in &instances[..2] {
        assert_eq!(i.display.colors[2].as_ref().unwrap().value, [9, 8, 7]);
        assert_eq!(i.display.colors[4].as_ref().unwrap().value, [1, 2, 3]);
        assert!(i.display.visibility[2].as_ref().unwrap().value);
        assert!(!i.display.visibility[4].as_ref().unwrap().value);
        assert!(i.display.colors[0].is_none());
    }
    assert_eq!(&instances[1].world_transform[12..15], &[10., 20., 30.]);
    assert_eq!(&instances[2].world_transform[12..15], &[12., 20., 30.]);
    assert!(instances[2].display.colors.iter().all(Option::is_none));
}

#[test]
fn framing_boundaries_limits_and_nonfinite_points() {
    let body = text(5, b"A\0", true);
    let data = [b";; HSF V19.10 \n".as_slice(), &body, b"\0"].concat();
    for cut in 0..data.len() {
        assert_eq!(
            parse(
                &data[..cut],
                &ParseOptions::default(),
                &mut Budget::new(Limits::default())
            )
            .unwrap_err()
            .diagnostic
            .classification,
            Status::Invalid
        );
    }
    for options in [
        ParseOptions {
            max_vertices: 0,
            ..Default::default()
        },
        ParseOptions {
            max_string_bytes: 1,
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
    for options in [
        SceneOptions {
            max_primitives: 0,
            ..Default::default()
        },
        SceneOptions {
            max_instances: 0,
            ..Default::default()
        },
    ] {
        let mut budget = Budget::new(Limits::default());
        let parsed = parse(&data, &ParseOptions::default(), &mut budget).unwrap();
        assert_eq!(
            scene(&parsed, &options, &mut budget)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::LimitExceeded
        );
    }
    let mut b = body;
    *b.last_mut().unwrap() = 2; // Character attributes remain unsupported.
    b.extend(b"(\x01x)");
    let result = read("19.10", &b, ParseMode::Partial).unwrap();
    assert_eq!(result.diagnostics[0].code, "hsf.text_options");
    assert!(result.data.primitives.is_empty());
    assert_eq!(result.data.nodes.len(), 1);
    let b = [b"l".as_slice(), &points(&[[f32::NAN, 0., 0.]; 2])].concat();
    assert_eq!(
        read("19.10", &b, ParseMode::Partial)
            .unwrap_err()
            .diagnostic
            .classification,
        Status::Invalid
    );
}
