use hsf_core::*;

fn parse_body(
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
fn render(mask: u32, high: u32, value: u32, high_value: u32, payload: &[u8]) -> Vec<u8> {
    let mut b = vec![b'R'];
    b.extend(mask.to_le_bytes());
    if mask & 0x80000000 != 0 {
        b.extend(high.to_le_bytes());
    }
    b.extend(value.to_le_bytes());
    if mask & 0x80000000 != 0 {
        b.extend(high_value.to_le_bytes());
    }
    b.extend(payload);
    b
}
fn color_locks(version: &str) -> Vec<u8> {
    let mut b = vec![];
    for n in [4u32, 4, 0x017f7f7f, 0x017f7f7f] {
        b.extend(n.to_le_bytes());
    }
    if version != "8.05" {
        let pairs = if version == "19.10" { 18 } else { 17 };
        for i in 0..pairs {
            b.extend((i as u16).to_le_bytes());
            b.extend((0x100 + i as u16).to_le_bytes());
        }
    }
    render(0x100000, 0, 0x100000, 0, &b)
}
fn clip() -> Vec<u8> {
    [
        vec![b'O', 1],
        3u32.to_le_bytes().to_vec(),
        [0f32, 0., 0., 1., 0., 0., 0., 1., 0.]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect(),
    ]
    .concat()
}
fn indices() -> Vec<u8> {
    [
        vec![b'n'],
        [2i32, -2, 7, 123, -456]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect(),
    ]
    .concat()
}

#[test]
fn observed_clip_option_framing_keeps_effects_opaque() {
    let mut record = clip();
    record[1] = 5;
    let body = [record.clone(), vec![b'l'], vec![0; 24], vec![0]].concat();
    let p = parse_body(&body, "19.10", ParseMode::Strict).unwrap();
    let layout = p.data.records[0].clip_region.as_ref().unwrap();
    assert_eq!(layout.options, 5);
    assert_eq!(layout.point_count, 3);
    assert_eq!(p.data.records[1].source.offset, 15 + 42);
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert_eq!(s.data.primitives.len(), 1);
    assert_eq!(s.data.rendering_options[0].kind, "clip_region");
    assert_eq!(s.data.placement_status, Status::Unsupported);
    assert!(s.data.primitive_instances.is_empty());
    for version in ["8.05", "14.50"] {
        let p = parse_body(&body, version, ParseMode::Partial).unwrap();
        assert_eq!(p.diagnostics[0].code, "hsf.clip_region_options");
        assert!(p.data.records.is_empty());
    }
    for cut in 1..record.len() {
        assert_eq!(
            parse_body(&record[..cut], "19.10", ParseMode::Partial)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::Invalid
        );
    }
}
#[test]
fn size_units_inherit_across_reopen_and_reference() {
    let mut b = vec![b'(', 1, b'a', 0x0e];
    b.extend((-3f32).to_le_bytes());
    b.extend([4, b')', b'q', b's', 0, 0, 0, 0, b'l']);
    b.extend(
        [0f32, 0., 0., 1., 1., 1.]
            .iter()
            .flat_map(|x| x.to_le_bytes()),
    );
    b.extend([b')', b'<', 1, b'a', 0]);
    let p = parse_body(&b, "19.10", ParseMode::Strict).unwrap();
    let s = scene(
        &p,
        &SceneOptions {
            mode: ParseMode::Strict,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert_eq!(s.data.primitive_instances.len(), 2);
    for instance in &s.data.primitive_instances {
        let a = instance.display.edge_weight.as_ref().unwrap();
        assert_eq!(a.value.value, 3.);
        assert_eq!(a.value.units, "pixels");
        assert_eq!(a.source, p.data.records[1].source);
    }
    // Negative zero consumes no units; a positive value replaces inherited units.
    for value in [0f32, -0., 2.] {
        let p = parse_body(
            &[vec![0x0e], value.to_le_bytes().to_vec(), vec![0]].concat(),
            "8.05",
            ParseMode::Strict,
        )
        .unwrap();
        assert!(
            p.data.records[0]
                .edge_weight
                .as_ref()
                .unwrap()
                .units
                .is_none()
        );
        assert_eq!(p.data.records[0].source.length, 5);
    }
}
#[test]
fn names_patterns_alignment_clip_and_metadata_keep_exact_spans() {
    let b = [
        b"@\xff\x03a\0b-\xff\x02xy\x0d\x34\x12*\x3f".to_vec(),
        clip(),
        indices(),
        vec![0],
    ]
    .concat();
    let p = parse_body(&b, "19.10", ParseMode::Strict).unwrap();
    let r = &p.data.records;
    assert_eq!(
        p.data
            .source_bytes(r[0].named_style.as_ref().unwrap().name.as_ref().unwrap())
            .unwrap(),
        b"a\0b"
    );
    assert_eq!(r[2].named_style.as_ref().unwrap().index, Some(0x1234));
    assert_eq!(r[3].value, Some(0x3f));
    assert_eq!(r[4].clip_region.as_ref().unwrap().points.length, 36);
    let l = r[5].user_index.as_ref().unwrap();
    assert_eq!(
        p.data.source_bytes(&l.indices).unwrap(),
        [-2i32, 7]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        p.data.source_bytes(&l.values).unwrap(),
        [123i32, -456]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<_>>()
    );
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert_eq!(s.data.rendering_options.len(), 5);
    assert_eq!(s.data.user_indices.len(), 1);
    assert_eq!(
        s.data.user_indices[0].interpretation_status,
        Status::Unsupported
    );
    assert_eq!(s.data.placement_status, Status::Unsupported);
    assert_eq!(
        scene(
            &p,
            &SceneOptions {
                mode: ParseMode::Strict,
                ..Default::default()
            },
            &mut Budget::new(Limits::default())
        )
        .unwrap_err()
        .diagnostic
        .code,
        "hsf.rendering_semantics"
    );
    // Old numeric patterns consume two bytes even when their low byte is FF.
    let p = parse_body(b"-\xff\x01@\x06*\x10\0", "8.05", ParseMode::Strict).unwrap();
    assert_eq!(
        p.data.records[0].named_style.as_ref().unwrap().index,
        Some(511)
    );
}
#[test]
fn render_masks_channel_order_versions_and_disabled_values() {
    for version in ["8.05", "14.50", "19.10"] {
        let b = color_locks(version);
        let p = parse_body(&[b.clone(), vec![0]].concat(), version, ParseMode::Strict).unwrap();
        assert_eq!(p.data.records[0].source.length, b.len() as u64);
        let a = p.data.records[0].rendering.as_ref().unwrap();
        assert_eq!(
            a.fields.len(),
            if version == "8.05" {
                4
            } else if version == "14.50" {
                38
            } else {
                40
            }
        );
        if version == "19.10" {
            assert_eq!(a.fields[36].name, "cut_face_lock_mask");
        }
    }
    // HSR/TQ present when mask is set although the value is zero.
    let b = render(0xc3000, 0, 0, 0, &[0x71, 0x22]);
    let p = parse_body(&[b, vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
    assert_eq!(p.data.records[0].source.length, 11);
    // High and low words precede both value words; depth-range exists in real 8.05.
    let b = render(
        0x80000000,
        0x10,
        0,
        0x10,
        &[0f32, 0.001]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    let p = parse_body(&[b, vec![0]].concat(), "8.05", ParseMode::Strict).unwrap();
    assert_eq!(
        p.data.records[0].rendering.as_ref().unwrap().mask,
        0x1080000000
    );
    assert_eq!(
        p.data.records[0].rendering.as_ref().unwrap().fields[0].name,
        "depth_range"
    );
    // Color channel selectors use value independently of color mask.
    let payload = [4u32, 4, 0, 0x1000]
        .iter()
        .flat_map(|x| x.to_le_bytes())
        .chain([1, 0, 2, 0])
        .collect::<Vec<_>>();
    let b = render(0x100000, 0, 0x100000, 0, &payload);
    let p = parse_body(&[b, vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
    assert_eq!(
        p.data.records[0].rendering.as_ref().unwrap().fields[4].name,
        "face_lock_mask"
    );
    // Buffer size is bit 0; retention/color-depth bits add no operands.
    for bits in [1, 2, 4, 8, 15] {
        let mut payload = vec![bits, bits];
        if bits & 1 != 0 {
            payload.extend(55i32.to_le_bytes());
        }
        let b = render(0x800000, 0, 0x800000, 0, &payload);
        let p = parse_body(&[b.clone(), vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
        assert_eq!(p.data.records[0].source.length, b.len() as u64);
    }
}
#[test]
fn render_fixed_fields_and_legacy_cut_edge_have_exact_order() {
    let mask = 0x80000000
        | 0x3000
        | 0xc0000
        | 0x200000
        | 0x400000
        | 0x20000
        | 0x40000000
        | 0x800000
        | 0x10000;
    let high = 0x10 | 0x20 | 0x40 | 0x400 | 0x4000 | 0x8000 | 0x10000 | 0x20000;
    let mut payload = vec![0x71, 0x22];
    payload.extend(2i32.to_le_bytes());
    payload.extend([1f32, 2.].iter().flat_map(|x| x.to_le_bytes()));
    payload.extend(3i32.to_le_bytes());
    payload.extend([4f32, 5.].iter().flat_map(|x| x.to_le_bytes()));
    payload.extend([1, 1]);
    payload.extend(6i32.to_le_bytes());
    payload.extend([0f32, 1.].iter().flat_map(|x| x.to_le_bytes()));
    payload.push(7);
    payload.extend([8f32, 9.].iter().flat_map(|x| x.to_le_bytes()));
    payload.extend([10, 11, 12]);
    payload.extend([13i32, 14].iter().flat_map(|x| x.to_le_bytes()));
    payload.extend([15f32, 16., 17., 18.].iter().flat_map(|x| x.to_le_bytes()));
    payload.push(19);
    let body = render(mask, high, mask, high, &payload);
    let p = parse_body(
        &[body.clone(), vec![0]].concat(),
        "19.10",
        ParseMode::Strict,
    )
    .unwrap();
    let layout = p.data.records[0].rendering.as_ref().unwrap();
    assert_eq!(p.data.records[0].source.length, body.len() as u64);
    assert_eq!(
        layout
            .fields
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        [
            "hsr",
            "technology_quantization",
            "face_displacement",
            "fog_limits",
            "debug",
            "stereo_separation",
            "stereo_distance",
            "buffer_options_mask",
            "buffer_options_value",
            "buffer_size_limit",
            "depth_range",
            "mask_transform",
            "image_scale",
            "image_tint",
            "general_displacement",
            "join_cutoff_angle",
            "screen_range",
            "display_list_level",
        ]
    );
    for end in 0..body.len() {
        assert_eq!(
            parse_body(&body[..end], "19.10", ParseMode::Partial)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::Invalid
        );
    }
    // Before 15.55, cut-edge also selects the text-contrast pair.
    for (version, fields) in [("14.50", 8), ("19.10", 6)] {
        let mut payload = [4u32, 4, 0, 0x100000]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<_>>();
        payload.extend(vec![0; (fields - 4) * 2]);
        let p = parse_body(
            &[render(0x100000, 0, 0x100000, 0, &payload), vec![0]].concat(),
            version,
            ParseMode::Strict,
        )
        .unwrap();
        assert_eq!(
            p.data.records[0].rendering.as_ref().unwrap().fields.len(),
            fields
        );
    }
    // These observed 19.10 selectors have no channel operand in the SDK grammar.
    for selector in [0x2000000u32, 0x4000000] {
        let payload = [4, 4, selector, selector]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<_>>();
        let p = parse_body(
            &[render(0x100000, 0, 0x100000, 0, &payload), vec![0]].concat(),
            "19.10",
            ParseMode::Strict,
        )
        .unwrap();
        assert_eq!(p.data.records[0].source.length, 25);
        assert_eq!(
            parse_body(
                &render(0x100000, 0, 0x100000, 0, &payload),
                "14.50",
                ParseMode::Strict
            )
            .unwrap_err()
            .diagnostic
            .code,
            "hsf.rendering_color_lock_mask"
        );
    }
}
#[test]
fn every_truncation_is_invalid_and_unsupported_never_resynchronizes() {
    for b in [
        color_locks("19.10"),
        color_locks("14.50"),
        clip(),
        indices(),
        b"@\xff\x03abc".to_vec(),
        [vec![0x0e], (-2f32).to_le_bytes().to_vec(), vec![3]].concat(),
    ] {
        let version = if b == color_locks("14.50") {
            "14.50"
        } else {
            "19.10"
        };
        for end in 0..b.len() {
            for mode in [ParseMode::Strict, ParseMode::Partial] {
                assert_eq!(
                    parse_body(&b[..end], version, mode)
                        .unwrap_err()
                        .diagnostic
                        .classification,
                    Status::Invalid,
                    "cut={end}"
                );
            }
        }
    }
    // Third word (TKO_Rendo_Extended2) with an operand-bearing option: contour.
    let mut extended2 = vec![b'R'];
    for word in [0x80000000u32, 0x80000000, 8, 0, 0, 8] {
        extended2.extend(word.to_le_bytes());
    }
    for (b, version, code) in [
        (
            render(0x80000000, 0x40000000, 0, 0, &[]),
            "19.10",
            "hsf.rendering_options_mask",
        ),
        (extended2, "19.10", "hsf.rendering_options_mask"),
        (
            render(0x18000000, 0, 0, 0, &[]),
            "19.10",
            "hsf.rendering_options_mask",
        ),
        (
            render(0x80000000, 0x10000, 0, 0, &[]),
            "14.50",
            "hsf.rendering_options_version",
        ),
        (b"@\xff".to_vec(), "8.05", "hsf.marker_symbol_version"),
        (b"O\x80".to_vec(), "19.10", "hsf.clip_region_options"),
        (
            [vec![0x0e], (-1f32).to_le_bytes().to_vec(), vec![255]].concat(),
            "19.10",
            "hsf.edge_weight_units",
        ),
    ] {
        let body = [b, vec![b'h', 1, 0]].concat();
        let p = parse_body(&body, version, ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty());
        assert_eq!(p.diagnostics[0].code, code);
        assert_eq!(p.data.domains[0].parsed_bytes, p.data.header.byte_len);
        assert_eq!(
            parse_body(&body, version, ParseMode::Strict)
                .unwrap_err()
                .diagnostic
                .code,
            code
        );
    }
}
#[test]
fn nonfinite_counts_and_budgets_are_rejected_before_retaining_data() {
    let mut bad_clip = clip();
    bad_clip[6..10].copy_from_slice(&f32::NAN.to_le_bytes());
    for b in [
        bad_clip,
        [vec![0x0e], f32::INFINITY.to_le_bytes().to_vec()].concat(),
        render(
            0x400000,
            0,
            0x400000,
            0,
            &[f32::NAN, 0.]
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect::<Vec<_>>(),
        ),
        b"n\xff\xff\xff\xff".to_vec(),
    ] {
        for mode in [ParseMode::Partial, ParseMode::Strict] {
            assert_eq!(
                parse_body(&b, "19.10", mode)
                    .unwrap_err()
                    .diagnostic
                    .classification,
                Status::Invalid
            );
        }
    }
    for (body, options, code) in [
        (
            clip(),
            ParseOptions {
                max_vertices: 2,
                ..Default::default()
            },
            "hsf.limit_vertices",
        ),
        (
            indices(),
            ParseOptions {
                max_records: 1,
                ..Default::default()
            },
            "hsf.limit_user_indices",
        ),
        (
            b"@\xff\x03abc".to_vec(),
            ParseOptions {
                max_string_bytes: 2,
                ..Default::default()
            },
            "hsf.limit_string",
        ),
    ] {
        let e = parse(
            &[b";; HSF V19.10 \n".as_slice(), &body].concat(),
            &options,
            &mut Budget::new(Limits::default()),
        )
        .unwrap_err();
        assert_eq!(e.diagnostic.code, code);
    }
    let b = [b";; HSF V19.10 \n".to_vec(), color_locks("19.10"), vec![0]].concat();
    assert_eq!(
        parse(
            &b,
            &ParseOptions::default(),
            &mut Budget::new(Limits {
                max_output_bytes: 8192,
                ..Default::default()
            })
        )
        .unwrap_err()
        .diagnostic
        .classification,
        Status::LimitExceeded
    );
}

#[test]
fn basic_patterns_and_markers_are_inherited_with_original_sources() {
    let p = parse_body(
        b"-\x02\x21\r\x09\x12@\x09(\x01aX\0\0\0\0\0\0\0\0\0\0\0\0)\0",
        "19.10",
        ParseMode::Strict,
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
    let d = &s.data.primitive_instances[0].display;
    let a = d.line_pattern.as_ref().unwrap();
    assert_eq!(a.value.index, Some(0x2102));
    assert_eq!(a.value.pattern.as_deref(), Some("dashed"));
    assert_eq!(a.value.cap.as_deref(), Some("round"));
    assert_eq!(a.value.join.as_deref(), Some("bevel"));
    assert_eq!(p.data.source_bytes(&a.source).unwrap(), b"-\x02\x21");
    assert_eq!(
        d.edge_pattern.as_ref().unwrap().value.pattern.as_deref(),
        Some("fine_dotted")
    );
    assert_eq!(
        d.marker_symbol.as_ref().unwrap().value.symbol.as_deref(),
        Some("star")
    );
    assert!(
        s.data
            .rendering_options
            .iter()
            .all(|r| r.interpretation_status == Status::Complete)
    );
}

#[test]
fn unknown_pattern_replaces_local_state_but_keeps_other_components() {
    let p = parse_body(
        b"-\0\0-\x0a\x21@\x09@\xff\x04star\0",
        "19.10",
        ParseMode::Strict,
    )
    .unwrap();
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    let d = &s.data.nodes[0].display;
    let a = &d.line_pattern.as_ref().unwrap().value;
    assert_eq!(a.index, Some(0x210a));
    assert!(a.pattern.is_none());
    assert_eq!(a.cap.as_deref(), Some("round"));
    assert_eq!(a.join.as_deref(), Some("bevel"));
    assert_eq!(a.interpretation_status, Status::Unsupported);
    let a = &d.marker_symbol.as_ref().unwrap().value;
    assert!(a.symbol.is_none() && a.index.is_none());
    assert_eq!(a.name.as_ref().unwrap().value.as_deref(), Some("star"));
    assert_eq!(s.data.placement_status, Status::Unsupported);
}
