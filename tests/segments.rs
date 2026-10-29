use hsf_core::*;
fn named(opcode: u8, name: &[u8]) -> Vec<u8> {
    [vec![opcode, name.len() as u8], name.to_vec()].concat()
}
fn read(body: &[u8], mode: ParseMode, max: u64) -> Result<ParseOutcome<Scene>, HsfError> {
    let bytes = [b";; HSF V19.10 \n".as_slice(), body, b"\0"].concat();
    let mut budget = Budget::new(Limits::default());
    let parsed = parse(
        &bytes,
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
            max_definitions: max,
            ..Default::default()
        },
        &mut budget,
    )
}
#[test]
fn aliases_are_assumptions_not_verified_placements() {
    for alias in [b"?Home".as_slice(), b"?Include Library", b"?Style Library"] {
        let body = [named(b'(', alias), b")".to_vec()].concat();
        let s = read(&body, ParseMode::Partial, 10).unwrap();
        assert_eq!(s.data.segment_aliases.len(), 1);
        assert_eq!(s.data.placement_status, Status::Unsupported);
        assert_eq!(s.diagnostics[0].severity, "warning");
        assert_eq!(
            read(&body, ParseMode::Strict, 10)
                .unwrap_err()
                .diagnostic
                .code,
            "hsf.segment_alias_default"
        );
        assert_eq!(
            read(&body, ParseMode::Partial, 0)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::LimitExceeded
        );
    }
}
#[test]
fn literal_identity_and_external_root_boundary() {
    let body = [
        named(b'(', b"/A  B/C"),
        b")".to_vec(),
        named(b'(', b"/a b/ c "),
        b")".to_vec(),
    ]
    .concat();
    let s = read(&body, ParseMode::Strict, 10).unwrap();
    assert_eq!(s.data.nodes.len(), 3);
    assert_eq!(s.status, Status::Complete);
    for path in [
        b"../a".as_slice(),
        b"?Home/../a",
        b"?unknown",
        b"/a/....",
        b"/a/`b`",
        b"/a,(b)",
    ] {
        let body = [named(b'(', path), b")".to_vec()].concat();
        let s = read(&body, ParseMode::Partial, 10).unwrap();
        assert_eq!(s.data.processed_records, 0);
        assert_eq!(s.diagnostics.last().unwrap().code, "hsf.segment_path");
    }
}
#[test]
fn styles_retain_edges_without_traversal_or_forward_resolution() {
    let body = [
        named(b'{', b"/later"),
        named(b'(', b"later"),
        named(b'{', b"/"),
        named(b'{', b"."),
        b")".to_vec(),
    ]
    .concat();
    let s = read(&body, ParseMode::Partial, 10).unwrap();
    assert_eq!(s.data.processed_records, 6);
    let links = s.data.style_segments;
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].target_id, None);
    assert_eq!(links[1].target_id.as_deref(), Some("segment:0"));
    assert_eq!(links[2].target_id.as_deref(), Some("segment:1"));
    assert!(s.data.references.is_empty());
    assert!(s.data.instances.is_empty());
    assert_eq!(
        read(&body, ParseMode::Strict, 10)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.style_unresolved"
    );
}
#[test]
fn alias_and_style_share_cumulative_definition_budget() {
    let body = named(b'{', b"?Home");
    let s = read(&body, ParseMode::Partial, 2).unwrap();
    assert_eq!(s.data.segment_aliases.len(), 1);
    assert_eq!(s.data.style_segments.len(), 1);
    assert_eq!(
        read(&body, ParseMode::Partial, 1)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.limit_definitions"
    );
}

fn read_with(body: &[u8], options: SceneOptions) -> Result<ParseOutcome<Scene>, HsfError> {
    let bytes = [b";; HSF V19.10 \n".as_slice(), body, b"\0"].concat();
    let mut budget = Budget::new(Limits::default());
    let parsed = parse(&bytes, &ParseOptions::default(), &mut budget)?;
    scene(&parsed, &options, &mut budget)
}
fn mounted() -> SceneOptions {
    SceneOptions {
        mount_path: Some("/include library/model_0_include".into()),
        ..Default::default()
    }
}
#[test]
fn mount_path_rebases_root_escaping_paths_into_the_file() {
    // The producer mounted the file root at /include library/model_0_include.
    // From /a/b, three steps up reach the mount's parent, and
    // model_0_include/shared/body0 returns into this file as /shared/body0.
    let body = [
        named(b'(', b"a"),
        named(b'(', b"b"),
        named(b'(', b"../../../model_0_include/shared/body0"),
        b")".to_vec(),
        named(b'<', b"../../../Model_0_Include/shared/body0"),
        b"))".to_vec(),
    ]
    .concat();
    let s = read_with(&body, mounted()).unwrap();
    assert_eq!(s.status, Status::Complete);
    let names: Vec<_> = s
        .data
        .nodes
        .iter()
        .map(|n| n.name.as_deref().unwrap_or(""))
        .collect();
    // The final node keeps its source spelling; its parent chain is the file path.
    assert_eq!(
        names,
        [
            "/",
            "a",
            "b",
            "shared",
            "../../../model_0_include/shared/body0"
        ]
    );
    assert_eq!(
        s.data.nodes[3].parent_id.as_deref(),
        Some(s.data.nodes[0].id.as_str())
    );
    assert!(s.data.nodes[3].implicit);
    assert_eq!(s.data.references.len(), 1);
    assert_eq!(
        s.data.references[0].target_id.as_deref(),
        Some(s.data.nodes[4].id.as_str())
    );
    // Paths that stay inside the file never touch the mount.
    let body = [named(b'(', b"a"), named(b'(', b"../c"), b"))".to_vec()].concat();
    let s = read_with(&body, mounted()).unwrap();
    assert_eq!(s.data.nodes[2].name.as_deref(), Some("../c"));
    assert_eq!(
        s.data.nodes[2].parent_id.as_deref(),
        Some(s.data.nodes[0].id.as_str())
    );
}
#[test]
fn mount_path_external_targets_and_invalid_mounts_are_explicit() {
    // Opening a segment under another mounted model is not representable.
    let body = [
        named(b'(', b"a"),
        named(b'(', b"../../model_7_include/shared/body0"),
        b"))".to_vec(),
    ]
    .concat();
    let s = read_with(&body, mounted()).unwrap();
    assert_eq!(s.data.processed_records, 1);
    assert_eq!(
        s.diagnostics.last().unwrap().code,
        "hsf.segment_path_external"
    );
    // An external include omits its branch and the scene continues.
    let body = [
        named(b'(', b"a"),
        named(b'<', b"../../model_7_include/x"),
        b")".to_vec(),
    ]
    .concat();
    let s = read_with(&body, mounted()).unwrap();
    assert_eq!(s.data.processed_records, 4);
    assert!(
        s.diagnostics
            .iter()
            .any(|d| d.code == "hsf.reference_external")
    );
    assert_eq!(s.data.references.len(), 1);
    assert_eq!(s.data.references[0].target_id, None);
    // Above the runtime root, or without a mount, escaping stays unsupported.
    let body = [named(b'(', b"../../../x"), b")".to_vec()].concat();
    for options in [mounted(), SceneOptions::default()] {
        let s = read_with(&body, options).unwrap();
        assert_eq!(s.data.processed_records, 0);
        assert_eq!(s.diagnostics.last().unwrap().code, "hsf.segment_path");
    }
    for mount in ["relative", "/a/../b", "/?Home", "/a//b", ""] {
        let e = read_with(
            b"",
            SceneOptions {
                mount_path: Some(mount.into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.diagnostic.code, "hsf.invalid_mount_path");
    }
}

#[test]
fn include_library_targets_render_only_through_includes() {
    // Outside the library, HOOPS renders an included segment at its own position
    // and again through the include.
    let shell = {
        let mut b = vec![b'S', 16, 0];
        b.extend(3i32.to_le_bytes());
        for v in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
            b.extend(v.to_le_bytes());
        }
        b.extend([1, 5, 0, 0, 0, 8, 3, 0, 1, 2]);
        b
    };
    let body = [
        named(b'(', b"defs"),
        shell.clone(),
        b")".to_vec(),
        named(b'(', b"x"),
        named(b'<', b"/defs"),
        b")".to_vec(),
    ]
    .concat();
    let s = read(&body, ParseMode::Strict, 10).unwrap();
    assert_eq!(s.data.instances.len(), 2);
    // Inside the mounted Include Library the target is a definition: one instance,
    // reached through the include, and the scene stays verified and complete.
    let body = [
        named(b'(', b"shared"),
        named(b'(', b"body0"),
        shell.clone(),
        b"))".to_vec(),
        named(b'(', b"a"),
        named(b'<', b"../../model_0_include/shared/body0"),
        b")".to_vec(),
    ]
    .concat();
    let s = read_with(&body, mounted()).unwrap();
    assert_eq!(s.status, Status::Complete);
    assert_eq!(s.data.instances.len(), 1);
    assert_eq!(s.data.instances[0].node_id, s.data.nodes[2].id);
    assert!(s.data.instances[0].path.contains(&s.data.references[0].id));
    // A library segment that nothing includes is not pruned.
    let body = [
        named(b'(', b"shared"),
        named(b'(', b"body0"),
        shell,
        b"))".to_vec(),
    ]
    .concat();
    let s = read_with(&body, mounted()).unwrap();
    assert_eq!(s.data.instances.len(), 1);
}
