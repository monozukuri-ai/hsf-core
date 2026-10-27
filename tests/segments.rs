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
