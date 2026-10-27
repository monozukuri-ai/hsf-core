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
fn scene_body(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<Scene>, HsfError> {
    scene(
        &parsed(body, "19.10", mode)?,
        &SceneOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn line() -> Vec<u8> {
    [vec![b'l'], floats(&[1., 2., 3., 4., 5., 6.])].concat()
}
fn heuristics() -> Vec<u8> {
    let mask = 0x016180ccu32;
    let value = mask & !0x200000; // weights depend on mask, not enabled value
    let mut b = vec![b'H'];
    b.extend(mask.to_le_bytes());
    b.extend(value.to_le_bytes());
    b.extend(9i32.to_le_bytes());
    b.extend((-1i32).to_le_bytes());
    b.extend([3, 0x87, 0x0e]);
    for n in [10i32, 20, 2] {
        b.extend(n.to_le_bytes());
    }
    b.push(0x25);
    b.extend(floats(&[1., 2., 3.]));
    b.extend(11i32.to_le_bytes());
    b.push(2);
    b.extend(floats(&[0., 1., 0., 0.01]));
    b
}
fn text_font() -> Vec<u8> {
    let mut b = b"F\xff\xff\xff\xff\x7f\0\x7f\0\x03abc".to_vec();
    for (value, unit) in [(1., 0u8), (0.1, 5)] {
        b.extend(floats(&[value]));
        b.push(unit);
    }
    b.push(1);
    b.extend(floats(&[-1., 10., 1.]));
    b.extend(floats(&[0.1]));
    b.push(0);
    b.extend(floats(&[1.2, 3.]));
    b.push(4);
    b.push(2);
    for n in [0x21, 0x10] {
        b.push(n);
        b.extend(floats(&[4.]));
        b.push(4);
    }
    b
}
fn material() -> Vec<u8> {
    let mut b = b"\"\x81\x80\x80\x01\xff\x03".to_vec();
    for color in 1..=5 {
        b.extend([0, color, color, color]);
    }
    b.extend(floats(&[12., 1.5]));
    b.extend(b"\0\x01b");
    b
}
#[test]
fn rendering_field_order_lengths_and_opaque_values_are_exact() {
    for b in [heuristics(), text_font(), material()] {
        let p = parsed(&[b.clone(), vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
        let r = &p.data.records[0];
        assert_eq!(r.source.length, b.len() as u64);
        let a = r.rendering.as_ref().unwrap();
        assert!(!a.fields.is_empty());
        for f in &a.fields {
            assert!(p.data.source_bytes(&f.source).is_some());
        }
        assert_eq!(
            r.source.offset + r.source.length,
            p.data.records[1].source.offset
        );
        let s = scene(
            &p,
            &SceneOptions::default(),
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        assert_eq!(s.data.rendering_options.len(), 1);
        assert_eq!(s.data.placement_status, Status::Unsupported);
        assert_eq!(s.status, Status::Partial);
    }
    let p = parsed(
        &[heuristics(), vec![0]].concat(),
        "19.10",
        ParseMode::Strict,
    )
    .unwrap();
    let l = p.data.records[0].rendering.as_ref().unwrap();
    assert_eq!(l.mask, 0x016180cc);
    assert_eq!(l.value, 0x014180cc);
    assert_eq!(
        l.fields
            .iter()
            .find(|f| f.name == "ordered_weight_5")
            .unwrap()
            .source
            .length,
        4
    );
    let p = parsed(&[material(), vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
    let l = p.data.records[0].rendering.as_ref().unwrap();
    assert_eq!(l.mask, 0x01808081);
    assert_eq!(l.value, 0x3ff);
    let env = l.fields.iter().find(|f| f.name == "environment").unwrap();
    assert_eq!(env.source.length, 0);
}
#[test]
fn truncations_unknown_masks_versions_and_nonfinite_values_fail_closed() {
    let records = [
        heuristics(),
        text_font(),
        material(),
        b"S\x90\x02\0\0".to_vec(),
        b"~\x81\x80\x80\x01\x01\x02\x03".to_vec(),
        [vec![b'='], floats(&[-2.]), vec![4]].concat(),
    ];
    for b in records {
        for cut in 0..b.len() {
            for mode in [ParseMode::Partial, ParseMode::Strict] {
                assert_eq!(
                    parsed(&b[..cut], "19.10", mode)
                        .unwrap_err()
                        .diagnostic
                        .classification,
                    Status::Invalid
                );
            }
        }
    }
    for (b, code) in [
        (b"H\0\x80\0\x80\0\0\0\0".to_vec(), "hsf.heuristics_mask"),
        (b"F\x80\0\x80\0\0\x80\0\0".to_vec(), "hsf.text_font_mask"),
        (b"\"\x01\x80\x04".to_vec(), "hsf.color_channels"),
    ] {
        let body = [b.clone(), vec![b'h', 1, 0]].concat();
        let p = parsed(&body, "19.10", ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty());
        assert_eq!(p.diagnostics[0].code, code);
        assert_eq!(
            parsed(&body, "19.10", ParseMode::Strict)
                .unwrap_err()
                .diagnostic
                .code,
            code
        );
    }
    for (b, code) in [
        (heuristics(), "hsf.heuristics_version"),
        (text_font(), "hsf.text_font_version"),
    ] {
        assert_eq!(
            parsed(&b, "8.05", ParseMode::Strict)
                .unwrap_err()
                .diagnostic
                .code,
            code
        );
    }
    for prefix in [b"F\x02\x02".as_slice(), b"\"\x01\x20"] {
        let b = [prefix, &f32::NAN.to_le_bytes(), &[0, 0]].concat();
        assert_eq!(
            parsed(&b, "19.10", ParseMode::Partial)
                .unwrap_err()
                .diagnostic
                .code,
            "hsf.nonfinite_rendering"
        );
    }
}
#[test]
fn null_shell_precedence_tag_identity_and_reference_do_not_fabricate_meshes() {
    for flags in [0x80, 0x90, 0x96, 0xff] {
        let b = vec![b'S', flags, 2, 0, 3, b'q', b'r', 0, 0, 0, 0, 0];
        let p = parsed(&b, "19.10", ParseMode::Strict).unwrap();
        let l = p.data.records[0].shell.as_ref().unwrap();
        assert!(l.null);
        assert_eq!(l.lod, 3);
        assert_eq!(l.vertices.length, 0);
        let s = scene_body(&b, ParseMode::Strict).unwrap();
        assert_eq!(s.data.null_shells.len(), 1);
        assert_eq!(
            s.data.references[0].target_id.as_deref(),
            Some("null_shell:0")
        );
        assert!(s.data.meshes.is_empty() && s.data.instances.is_empty());
        assert_eq!(s.status, Status::Complete);
    }
    let p = parsed(b"S\x90\x02\0\0\0", "19.10", ParseMode::Strict).unwrap();
    let e = scene(
        &p,
        &SceneOptions {
            max_meshes: 0,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_meshes");
}
#[test]
fn literal_paths_create_parents_and_close_restores_the_previous_scope() {
    // Quoted path grammar is not treated as literal slash-separated names.
    let quoted = b"(\x08/a/'b/c')\0";
    assert_eq!(
        scene_body(quoted, ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.segment_path"
    );
    let mut b = b"(\x06/a/b/c".to_vec();
    b.extend(line());
    b.extend(b")");
    b.extend(line());
    b.push(0);
    let s = scene_body(&b, ParseMode::Strict).unwrap();
    assert_eq!(s.data.nodes.len(), 4);
    assert!(s.data.nodes[1].implicit && s.data.nodes[2].implicit);
    assert!(!s.data.nodes[3].implicit);
    assert!(s.data.nodes[1].name_source.is_none());
    assert_eq!(s.data.nodes[2].parent_id.as_deref(), Some("segment:1"));
    assert_eq!(
        s.data.primitive_instances[0].path,
        ["segment:0", "segment:1", "segment:2", "segment:3"]
    );
    assert_eq!(s.data.primitive_instances[1].node_id, "segment:0");
    let p = parsed(&b, "19.10", ParseMode::Strict).unwrap();
    for opts in [
        SceneOptions {
            max_nodes: 3,
            ..Default::default()
        },
        SceneOptions {
            max_depth: 3,
            ..Default::default()
        },
    ] {
        assert_eq!(
            scene(&p, &opts, &mut Budget::new(Limits::default()))
                .unwrap_err()
                .diagnostic
                .classification,
            Status::LimitExceeded
        );
    }
}
#[test]
fn extended_rgb_and_line_weight_inherit_and_render_settings_withhold_placements() {
    let mut b = b"~\x81\x80\x80\x01\x01\x02\x03".to_vec();
    b.push(b'=');
    b.extend(floats(&[-2.]));
    b.push(4);
    b.extend(b"(\x01a");
    b.extend(line());
    b.extend(b")\0");
    let s = scene_body(&b, ParseMode::Strict).unwrap();
    let d = &s.data.primitive_instances[0].display;
    assert_eq!(d.colors[24].as_ref().unwrap().value, [1, 2, 3]);
    for bit in [7, 15, 23] {
        assert!(d.colors[bit].is_none());
    }
    assert_eq!(d.line_weight.as_ref().unwrap().value.units, "pixels");
    b.pop();
    b.extend(b"H\x04\0\x04\0\x01\0");
    let s = scene_body(&b, ParseMode::Partial).unwrap();
    assert!(s.data.primitive_instances.is_empty());
    assert_eq!(s.data.primitives.len(), 1);
    assert_eq!(
        scene_body(&b, ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.rendering_semantics"
    );
}
#[test]
fn diffuse_extended_lengths_and_resource_bounds_are_checked() {
    let s = scene_body(b"~\x01\xff\0\0\"\x01\x01\x03red\0", ParseMode::Partial).unwrap();
    assert!(s.data.nodes[0].face_color.is_none());
    assert!(s.data.nodes[0].display.colors[0].is_none());
    let mut b = b"\"\x01\x01\xff".to_vec();
    b.extend(300i32.to_le_bytes());
    b.extend(vec![b'x'; 300]);
    b.push(0);
    let p = parsed(&b, "19.10", ParseMode::Strict).unwrap();
    assert_eq!(
        p.data.records[0].rendering.as_ref().unwrap().fields[0]
            .source
            .length,
        300
    );
    let raw = [b";; HSF V19.10 \n".as_slice(), &b].concat();
    assert_eq!(
        parse(
            &raw,
            &ParseOptions {
                max_string_bytes: 299,
                ..Default::default()
            },
            &mut Budget::new(Limits::default())
        )
        .unwrap_err()
        .diagnostic
        .classification,
        Status::LimitExceeded
    );
    b[4..8].copy_from_slice(&(-1i32).to_le_bytes());
    assert_eq!(
        parsed(&b, "19.10", ParseMode::Partial)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.negative_count"
    );
}
