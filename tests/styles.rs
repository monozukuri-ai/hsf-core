use hsf_core::*;

fn named(op: u8, value: &str) -> Vec<u8> {
    [vec![op, value.len() as u8], value.as_bytes().to_vec()].concat()
}
fn color(value: u8) -> Vec<u8> {
    vec![b'~', 1, value, 2, 3]
}
fn line() -> Vec<u8> {
    [vec![b'l'], vec![0; 24]].concat()
}
fn parsed(body: &[u8]) -> ParseOutcome<HsfDocument> {
    parse(
        &[b";; HSF V19.10 \n".as_slice(), body, b"\0"].concat(),
        &ParseOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap()
}
fn read(body: &[u8], options: SceneOptions) -> Result<ParseOutcome<Scene>, HsfError> {
    scene(&parsed(body), &options, &mut Budget::new(Limits::default()))
}

#[test]
fn local_style_parent_order_and_final_reopen_source() {
    let body = [
        color(1),
        named(b'(', "lib"),
        b"V\x01\0".to_vec(),
        named(b'(', "base"),
        color(4),
        b"q))".to_vec(),
        named(b'(', "style"),
        named(b'{', "/lib/base"),
        b")".to_vec(),
        named(b'(', "part"),
        named(b'{', "/style"),
        line(),
        named(b'(', "child"),
        color(9),
        line(),
        b"))s\0\0\0\0".to_vec(),
        color(7),
        b")".to_vec(),
    ]
    .concat();
    let p = parsed(&body);
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
    assert_eq!(s.data.style_resolutions.len(), 2);
    assert_eq!(s.data.primitive_instances.len(), 2);
    for (i, expected) in s.data.primitive_instances.iter().zip([7, 9]) {
        let a = i.display.colors[0].as_ref().unwrap();
        assert_eq!(a.value, [expected, 2, 3]);
        assert_eq!(p.data.source_bytes(&a.source).unwrap(), color(expected));
        assert!(i.display.visibility[0].is_none());
    }
    assert!(s.data.nodes[4].display.colors[0].is_none());
}

#[test]
fn conflicts_cycles_conditions_unresolved_and_non_display_effects_are_closed() {
    let bodies = [
        (
            [
                named(b'(', "a"),
                color(1),
                b")".to_vec(),
                named(b'(', "b"),
                color(1),
                b")".to_vec(),
                named(b'{', "/a"),
                named(b'{', "/b"),
            ]
            .concat(),
            "hsf.style_conflict",
        ),
        (named(b'{', "/"), "hsf.style_cycle"),
        (
            [named(b'{', "/later"), named(b'(', "later"), b")".to_vec()].concat(),
            "hsf.style_unresolved",
        ),
        (
            [named(b'(', "a"), b")".to_vec(), b"{\0\x02/a\0".to_vec()].concat(),
            "hsf.style_condition",
        ),
        (
            [named(b'(', "a"), b"h\x01)".to_vec(), named(b'{', "/a")].concat(),
            "hsf.style_target_attributes",
        ),
    ];
    for (mut body, code) in bodies {
        body.extend(line());
        let s = read(&body, SceneOptions::default()).unwrap();
        assert_eq!(s.data.style_resolutions[0].reason.as_deref(), Some(code));
        assert!(s.data.style_resolutions[0].display.is_none());
        assert!(s.data.primitive_instances.is_empty());
        assert_eq!(
            read(
                &body,
                SceneOptions {
                    mode: ParseMode::Strict,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .diagnostic
            .code,
            code
        );
    }
}

fn chain(count: usize, reverse: bool) -> Vec<u8> {
    let mut b = vec![];
    // All targets exist; dependencies are assigned in either cache order.
    for i in 0..count {
        b.extend(named(b'(', &format!("n{i}")));
        b.push(b')');
    }
    for j in 0..count {
        let i = if reverse { count - j - 1 } else { j };
        b.extend(named(b'(', &format!("n{i}")));
        if i == 0 {
            b.extend(color(1));
        } else {
            b.extend(named(b'{', &format!("/n{}", i - 1)));
        }
        b.push(b')');
    }
    b.extend(named(b'{', &format!("/n{}", count - 1)));
    b.extend(line());
    b
}

#[test]
fn cache_order_cannot_bypass_graph_depth_and_budget() {
    for reverse in [true, false] {
        let body = chain(32, reverse);
        let e = read(
            &body,
            SceneOptions {
                max_depth: 32,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.diagnostic.code, "hsf.limit_scene_depth");
        assert_eq!(e.diagnostic.stage, "limits");
        let s = read(
            &body,
            SceneOptions {
                max_depth: 33,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(s.status, Status::Complete);
        assert_eq!(s.data.primitive_instances.len(), 1);
        let e = scene(
            &parsed(&body),
            &SceneOptions::default(),
            &mut Budget::new(Limits {
                max_output_bytes: 350_000,
                ..Default::default()
            }),
        )
        .unwrap_err();
        assert_eq!(e.diagnostic.classification, Status::LimitExceeded);
    }
}

#[test]
fn visits_are_shared_with_geometry_traversal() {
    let body = [
        named(b'(', "a"),
        color(1),
        b")".to_vec(),
        named(b'{', "/a"),
        line(),
    ]
    .concat();
    assert_eq!(
        read(
            &body,
            SceneOptions {
                max_visits: 2,
                ..Default::default()
            }
        )
        .unwrap_err()
        .diagnostic
        .code,
        "hsf.limit_scene_visits"
    );
    assert_eq!(
        read(
            &body,
            SceneOptions {
                max_visits: 3,
                ..Default::default()
            }
        )
        .unwrap()
        .status,
        Status::Complete
    );
}

#[test]
fn syntax_and_semantic_prefixes_never_publish_final_style_values() {
    for suffix in [
        vec![255],
        [named(b'(', "../outside"), b")".to_vec()].concat(),
    ] {
        let body = [
            named(b'(', "a"),
            color(1),
            b")".to_vec(),
            named(b'{', "/a"),
            suffix,
        ]
        .concat();
        let s = read(&body, SceneOptions::default()).unwrap();
        assert_eq!(
            s.data.style_resolutions[0].reason.as_deref(),
            Some("hsf.style_incomplete_scene")
        );
        assert!(s.data.style_resolutions[0].display.is_none());
    }
}
