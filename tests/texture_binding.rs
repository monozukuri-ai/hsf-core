use hsf_core::*;

fn named(op: u8, name: &str) -> Vec<u8> {
    [vec![op, name.len() as u8], name.as_bytes().to_vec()].concat()
}
fn color(text: &str) -> Vec<u8> {
    [vec![b'"', 1, 1, text.len() as u8], text.as_bytes().to_vec()].concat()
}
fn use_texture() -> Vec<u8> {
    color("diffuse color=`r=0.25 g=0.5 b=1`,diffuse texture=(n)")
}
fn image() -> Vec<u8> {
    // Hand-authored marker envelope, independently of the Python pixel fixture.
    let jpeg = [
        255, 216, 255, 192, 0, 17, 8, 0, 1, 0, 3, 3, 1, 17, 0, 2, 17, 0, 3, 17, 0, 255, 218, 0, 12,
        3, 1, 0, 2, 0, 3, 0, 0, 63, 0, 23, 255, 0, 42, 255, 217,
    ];
    let mut b = vec![b'i'];
    b.extend([0; 12]);
    b.extend(0xa3u32.to_le_bytes());
    b.extend([1, b'n']);
    b.extend(3u32.to_le_bytes());
    b.extend(1u32.to_le_bytes());
    b.push(2);
    b.extend((jpeg.len() as u32).to_le_bytes());
    b.extend(jpeg);
    b.extend([0, 255]);
    b
}
fn parse_body(body: &[u8]) -> ParseOutcome<HsfDocument> {
    parse(
        &[b";; HSF V19.10 \n".as_slice(), body, b"\0"].concat(),
        &ParseOptions {
            image_profile: ImageProfile::U32RgbaJpegV1,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap()
}
fn read(body: &[u8], options: SceneOptions) -> Result<ParseOutcome<Scene>, HsfError> {
    scene(
        &parse_body(body),
        &options,
        &mut Budget::new(Limits::default()),
    )
}

#[test]
fn nested_style_and_later_image_keep_substring_sources_and_original_events() {
    let body = [
        named(b'(', "a"),
        b"t\x01n\x01n\x01\0\x01)".to_vec(),
        named(b'(', "b"),
        named(b'{', "/a"),
        b")".to_vec(),
        named(b'{', "/b"),
        use_texture(),
        named(b'(', "a"),
        image(),
        b")".to_vec(),
    ]
    .concat();
    let p = parse_body(&body);
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    let u = &s.data.texture_uses[0];
    assert_eq!(u.binding.status, Status::Complete);
    assert_eq!(u.binding.image_id.as_deref(), Some("image:0"));
    assert_eq!(u.binding.style_path, ["style_segment:1", "style_segment:0"]);
    assert_eq!(u.binding.texture_definition_ids, ["texture:0"]);
    assert_eq!(
        p.data
            .source_bytes(&u.texture_name.as_ref().unwrap().source),
        Some(b"n".as_slice())
    );
    let c = u.diffuse_color.as_ref().unwrap();
    assert_eq!(c.value, [0.25, 0.5, 1.]);
    assert_eq!(
        p.data.source_bytes(&c.source),
        Some(b"r=0.25 g=0.5 b=1".as_slice())
    );
    assert_eq!(u.interpretation_status, Status::Unsupported);
    assert_eq!(s.data.placement_status, Status::Unsupported);
    assert!(s.data.instances.is_empty());
}

#[test]
fn unknown_expressions_are_not_partially_accepted() {
    for expression in [
        "diffuse texture=n,garbage",
        "diffuse texture=(n,x)",
        "diffuse texture='n'",
        "diffuse color=`r=NaN g=0 b=1`,diffuse texture=n",
        "diffuse color=`r=1g=0 b=1`,diffuse texture=n",
        "diffuse texture=n\0",
    ] {
        let s = read(
            &[color(expression), image()].concat(),
            SceneOptions::default(),
        )
        .unwrap();
        let u = &s.data.texture_uses[0];
        assert!(u.texture_name.is_none());
        assert_eq!(u.binding.reason.as_deref(), Some("hsf.texture_expression"));
        assert!(u.binding.image_id.is_none());
    }
}

#[test]
fn ambiguity_cycles_conditions_and_other_scopes_fail_closed() {
    let cases = [
        ([image(), image()].concat(), "hsf.texture_ambiguous_image"),
        (
            [named(b'(', "a"), image(), b")".to_vec()].concat(),
            "hsf.texture_parent_context",
        ),
        (
            [b"t\x01n\x01x\0\0".to_vec(), image()].concat(),
            "hsf.texture_definition_source",
        ),
        (named(b'{', "/"), "hsf.texture_style_cycle"),
        (named(b'{', "/none"), "hsf.texture_style_unresolved"),
        (
            [named(b'(', "a"), image(), b"){\0\x02/a\0".to_vec()].concat(),
            "hsf.texture_style_condition",
        ),
    ];
    for (body, code) in cases {
        let s = read(&[body, use_texture()].concat(), SceneOptions::default()).unwrap();
        let b = &s.data.texture_uses[0].binding;
        assert_eq!(b.reason.as_deref(), Some(code));
        assert!(b.image_id.is_none());
    }
}

#[test]
fn style_lookup_depth_visits_and_output_are_bounded() {
    let mut body = vec![];
    for i in 0..10 {
        body.extend(named(b'(', &format!("n{i}")));
        body.push(b')');
    }
    for i in 0..10 {
        body.extend(named(b'(', &format!("n{i}")));
        if i == 0 {
            body.extend(image());
        } else {
            body.extend(named(b'{', &format!("/n{}", i - 1)));
        }
        body.push(b')');
    }
    body.extend(named(b'{', "/n9"));
    body.extend(use_texture());
    let e = read(
        &body,
        SceneOptions {
            max_depth: 10,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_scene_depth");
    let s = read(
        &body,
        SceneOptions {
            max_depth: 11,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(s.data.texture_uses[0].binding.style_path.len(), 10);
    let p = parse_body(&body);
    let e = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits {
            max_output_bytes: 100_000,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.classification, Status::LimitExceeded);
    let e = read(
        &[image(), use_texture(), use_texture()].concat(),
        SceneOptions {
            max_visits: 1,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_scene_visits");
}

#[test]
fn incomplete_prefix_invalidates_preceding_links() {
    for tail in [
        vec![255],
        [named(b'(', "../outside"), b")".to_vec()].concat(),
    ] {
        let s = read(
            &[image(), use_texture(), tail].concat(),
            SceneOptions::default(),
        )
        .unwrap();
        let b = &s.data.texture_uses[0].binding;
        assert_eq!(b.reason.as_deref(), Some("hsf.texture_incomplete_scene"));
        assert!(b.image_id.is_none());
    }
}
