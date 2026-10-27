use hsf_core::*;
fn read(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<Scene>, HsfError> {
    let mut budget = Budget::new(Limits::default());
    let parsed = parse(
        &[b";; HSF V19.10 \n".as_slice(), body, &[0]].concat(),
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
#[test]
fn independent_text_components_no_setting_and_unknown_values() {
    let result = read(b"*\x25*\x1f*\x03", ParseMode::Strict).unwrap();
    let d = &result.data.nodes[0].display;
    assert_eq!(
        d.text_anchor.as_ref().unwrap().value.name.as_deref(),
        Some("lower_right")
    );
    assert_eq!(
        d.text_justification.as_ref().unwrap().value.name.as_deref(),
        Some("left")
    );
    let result = read(b"*\xa5", ParseMode::Partial).unwrap();
    assert_eq!(
        result.data.nodes[0]
            .display
            .text_justification
            .as_ref()
            .unwrap()
            .value
            .index,
        10
    );
    assert_eq!(result.data.placement_status, Status::Unsupported);
}
#[test]
fn glyph_wire_program_and_forward_marker_binding() {
    let result = read(
        b"@\xff\x03ownj\x03own\x09\0\x0a\0\0\x02\xf6\0\x0a\0\0",
        ParseMode::Strict,
    )
    .unwrap();
    let glyph = &result.data.glyphs[0];
    assert_eq!(
        glyph.program.as_ref().unwrap().commands[0].points,
        [[-10, 0], [10, 0]]
    );
    let marker = &result.data.nodes[0]
        .display
        .marker_symbol
        .as_ref()
        .unwrap()
        .value;
    assert_eq!(marker.symbol.as_deref(), Some("custom"));
    assert_eq!(
        marker.binding.as_ref().unwrap().definition_id.as_ref(),
        Some(&glyph.id)
    );
    assert_eq!(result.status, Status::Complete);
}
#[test]
fn font_typed_scalar_and_units_do_not_claim_font_resolution() {
    let body = [b"F\x02\x02".as_slice(), &2.5f32.to_le_bytes(), &[4]].concat();
    let result = read(&body, ParseMode::Partial).unwrap();
    let settings = result.data.rendering_options[0].settings.as_ref().unwrap();
    assert_eq!(settings.operands[0].floats, [2.5]);
    assert_eq!(
        settings.operands[1]
            .enumeration
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("pixels")
    );
    assert_eq!(settings.decoding_status, Status::Complete);
    assert_eq!(settings.interpretation_status, Status::Unsupported);
    assert_eq!(result.data.placement_status, Status::Unsupported);
}
