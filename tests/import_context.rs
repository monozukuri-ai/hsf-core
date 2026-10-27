use hsf_core::*;
fn named(op: u8, s: &[u8]) -> Vec<u8> {
    [vec![op, s.len() as u8], s.to_vec()].concat()
}
fn read(body: &[u8], options: SceneOptions) -> Result<ParseOutcome<Scene>, HsfError> {
    let mut budget = Budget::new(Limits::default());
    let parsed = parse(
        &[b";; HSF V19.10 \n".as_slice(), body, &[0]].concat(),
        &ParseOptions::default(),
        &mut budget,
    )?;
    scene(&parsed, &options, &mut budget)
}
fn context(root: &str) -> SceneOptions {
    SceneOptions {
        import_root: Some(root.into()),
        ..Default::default()
    }
}
#[test]
fn explicit_namespace_resolves_sibling_without_rebasing_absolute_paths() {
    let body = [
        vec![b'h', 0],
        named(b'(', b"a/b"),
        named(b'(', b"../../../shared"),
        vec![b')'],
        named(b'<', b"../../../shared"),
        vec![b')'],
        named(b'(', b"/shared"),
        vec![b')'],
    ]
    .concat();
    let default = read(&body, SceneOptions::default()).unwrap();
    assert_eq!(default.data.processed_records, 2);
    assert_eq!(default.diagnostics.last().unwrap().code, "hsf.segment_path");
    let s = read(&body, context("/import")).unwrap();
    assert_eq!(s.data.processed_records, 9);
    assert_eq!(s.data.nodes.len(), 5);
    assert_eq!(s.data.nodes[4].parent_id.as_deref(), Some("segment:0"));
    assert_eq!(s.data.references[0].target_id.as_deref(), Some("segment:4"));
    assert!(s.data.nodes[0].world_handedness.is_none());
    assert!(s.data.nodes[1].world_handedness.is_some());
    assert_eq!(s.data.placement_status, Status::Unsupported);
    assert_eq!(s.diagnostics[0].code, "hsf.import_context");
    assert_eq!(s.diagnostics[0].severity, "warning");
}
#[test]
fn context_provenance_has_no_fabricated_file_source() {
    let s = read(&[], context("/ Host /  My   Import ")).unwrap();
    let c = s.data.import_context.unwrap();
    assert_eq!(c.requested_root, "/ Host /  My   Import ");
    assert_eq!(c.resolved_root, "/host/my import");
    assert_eq!(c.node_id, "segment:2");
    assert_eq!(c.context_node_ids, ["segment:0", "segment:1", "segment:2"]);
    assert!(
        s.data
            .nodes
            .iter()
            .all(|n| n.source.is_none() && n.name_source.is_none())
    );
    assert_eq!(c.interpretation_status, Status::Unsupported);
    assert!(s.diagnostics[0].source.is_none());
}
#[test]
fn invalid_roots_and_strict_context_remain_explicit_errors() {
    for root in [
        "",
        "relative",
        "/..",
        "/a//b",
        "/a/",
        "/^",
        "/?Home",
        "/...",
        "/....",
        "/'a'",
        "/a\0b",
        "/日本語",
    ] {
        for mode in [ParseMode::Strict, ParseMode::Partial] {
            assert_eq!(
                read(
                    &[],
                    SceneOptions {
                        mode,
                        ..context(root)
                    }
                )
                .unwrap_err()
                .diagnostic
                .code,
                "hsf.invalid_import_root"
            );
        }
    }
    for root in ["/", "/import"] {
        assert_eq!(
            read(
                &[],
                SceneOptions {
                    mode: ParseMode::Strict,
                    ..context(root)
                }
            )
            .unwrap_err()
            .diagnostic
            .code,
            "hsf.import_context"
        );
    }
}
#[test]
fn context_does_not_clamp_or_load_external_targets_and_limits_include_ancestors() {
    let b = [named(b'(', b"../../outside"), vec![b')']].concat();
    let s = read(&b, context("/import")).unwrap();
    assert_eq!(s.data.processed_records, 0);
    assert_eq!(s.diagnostics.last().unwrap().code, "hsf.segment_path");
    let s = read(&named(b'<', b"../missing"), context("/import")).unwrap();
    assert!(s.data.references[0].target_id.is_none());
    assert_eq!(s.data.nodes.len(), 2);
    for options in [
        SceneOptions {
            max_nodes: 2,
            ..context("/a/b")
        },
        SceneOptions {
            max_depth: 2,
            ..context("/a/b")
        },
    ] {
        assert_eq!(
            read(&[], options).unwrap_err().diagnostic.classification,
            Status::LimitExceeded
        );
    }
}

#[test]
fn anonymous_children_do_not_bypass_context_depth_budget() {
    let s = read(
        &[b'(', 0, b')'],
        SceneOptions {
            max_depth: 2,
            ..context("/import")
        },
    );
    assert_eq!(s.unwrap_err().diagnostic.code, "hsf.limit_scene_depth");
}
