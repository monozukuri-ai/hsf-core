use hsf_core::*;

fn definition(program: &[u8]) -> Vec<u8> {
    [
        b"J\x06custom".as_slice(),
        &(program.len() as u16).to_le_bytes(),
        program,
    ]
    .concat()
}
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
fn forward_binding_retains_explicit_program_and_final_assignment() {
    let body = [
        b"-\xff\x06custom\r\xff\x06custom-\x02\x21".as_slice(),
        &definition(b"2 px dash,0.000635 oru blank"),
    ]
    .concat();
    let s = read(&body, ParseMode::Strict).unwrap();
    assert_eq!(s.status, Status::Complete);
    let definition = &s.data.line_styles[0];
    let program = definition.program.as_ref().unwrap();
    assert_eq!(program.kind, "explicit_dash_blank");
    assert_eq!(program.elements[0].length, Some(2.0));
    assert_eq!(program.elements[0].units.as_deref(), Some("pixels"));
    assert_eq!(program.elements[1].length, Some(0.000635));
    assert_eq!(
        program.elements[1].units.as_deref(),
        Some("object_relative")
    );
    let binding = s.data.rendering_options[0]
        .line_style_binding
        .as_ref()
        .unwrap();
    assert_eq!(binding.definition_id.as_ref(), Some(&definition.id));
    assert_eq!(binding.status, Status::Complete);
    let d = &s.data.nodes[0].display;
    assert!(d.line_pattern.as_ref().unwrap().value.binding.is_none());
    assert_eq!(
        d.line_pattern.as_ref().unwrap().value.pattern.as_deref(),
        Some("dashed")
    );
    assert_eq!(
        d.edge_pattern
            .as_ref()
            .unwrap()
            .value
            .binding
            .as_ref()
            .unwrap()
            .status,
        Status::Complete
    );
}

#[test]
fn collisions_incomplete_scenes_and_complex_programs_fail_closed() {
    for (body, expected) in [
        (
            [
                definition(b"1 px dash"),
                definition(b"1 px dash"),
                b"-\xff\x06custom".to_vec(),
            ]
            .concat(),
            "hsf.line_style_ambiguous",
        ),
        (
            [definition(b"1 px dash"), b"-\xff\x06custom\xfe".to_vec()].concat(),
            "hsf.line_style_incomplete_scene",
        ),
        (
            [
                definition(b"1 px dash, parallel"),
                b"-\xff\x06custom".to_vec(),
            ]
            .concat(),
            "hsf.line_style_program",
        ),
    ] {
        let s = read(&body, ParseMode::Partial).unwrap();
        assert_eq!(s.status, Status::Partial);
        assert_eq!(s.data.placement_status, Status::Unsupported);
        assert_eq!(
            s.data.rendering_options[0]
                .line_style_binding
                .as_ref()
                .unwrap()
                .reason
                .as_deref(),
            Some(expected)
        );
        assert!(read(&body, ParseMode::Strict).is_err());
    }
    assert!(
        read(&definition(b"parallel"), ParseMode::Partial)
            .unwrap()
            .data
            .line_styles[0]
            .program
            .is_none()
    );
}

#[test]
fn program_element_limit_is_fatal_in_partial_mode() {
    let program = vec!["1 px dash"; 257].join(",");
    let e = read(&definition(program.as_bytes()), ParseMode::Partial).unwrap_err();
    assert_eq!(e.diagnostic.classification, Status::LimitExceeded);
    assert_eq!(e.diagnostic.code, "hsf.limit_line_style_elements");
}

#[test]
fn explicit_parallel_modifiers_and_unsized_blank_preserve_operands() {
    let program =
        b"1 oru dash, parallel, -2 px fixed offset, 1.5 pixel weight, dash, blank, segmented";
    let body = [definition(program), b"-\xff\x06custom".to_vec()].concat();
    let result = read(&body, ParseMode::Strict).unwrap();
    let program = result.data.line_styles[0].program.as_ref().unwrap();
    assert_eq!(program.kind, "parallel_dash_blank");
    assert!(program.elements.is_empty());
    let first = &program.parallels[0];
    let second = &program.parallels[1];
    assert!(first.marker_source.is_none() && first.weight.is_none());
    let offset = second.offset.as_ref().unwrap();
    assert_eq!(offset.value, -2.0);
    assert!(offset.fixed && offset.fixed_source.is_some());
    assert_eq!(second.weight.as_ref().unwrap().value, 1.5);
    assert!(
        second
            .elements
            .iter()
            .all(|e| e.stretchable && e.length.is_none())
    );
    assert!(second.segmented_source.is_some());
    assert_eq!(
        result.data.rendering_options[0]
            .line_style_binding
            .as_ref()
            .unwrap()
            .status,
        Status::Complete
    );
}

#[test]
fn unitless_weights_are_decoded_but_not_assigned_an_inferred_unit() {
    let body = [
        definition(b"1 px dash, parallel, 1.5 weight, dash, blank, segmented"),
        b"-\xff\x06custom".to_vec(),
    ]
    .concat();
    let result = read(&body, ParseMode::Partial).unwrap();
    let def = &result.data.line_styles[0];
    let program = def.program.as_ref().unwrap();
    assert_eq!(program.interpretation_status, Status::Unsupported);
    assert_eq!(
        program.reason.as_deref(),
        Some("hsf.line_style_weight_units")
    );
    let weight = program.parallels[1].weight.as_ref().unwrap();
    assert!(weight.units.is_none() && weight.units_source.is_none());
    assert_eq!(weight.value, 1.5);
    assert_eq!(def.reason, program.reason);
    assert_eq!(result.data.placement_status, Status::Unsupported);
    assert_eq!(
        read(&body, ParseMode::Strict).unwrap_err().diagnostic.code,
        "hsf.line_style_definition"
    );
    let program = vec!["dash"; 33].join(", parallel, ");
    assert_eq!(
        read(&definition(program.as_bytes()), ParseMode::Partial)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.limit_line_style_parallels"
    );
}

#[test]
fn extended_contrast_fit_and_absolute_preserve_sources_and_binding() {
    let result = read(
        &[
            definition(b"fit=stretch, absolute, 2 px contrast, 1 px blank"),
            b"-\xff\x06custom".to_vec(),
        ]
        .concat(),
        ParseMode::Strict,
    )
    .unwrap();
    let p = result.data.line_styles[0].program.as_ref().unwrap();
    assert_eq!(p.kind, "extended_line_style");
    assert!(p.elements.is_empty() && p.transformed_source.is_none());
    let parallel = &p.parallels[0];
    assert_eq!(parallel.fit.as_ref().unwrap().mode, "stretch");
    assert_eq!(
        parallel.absolute_length_source,
        parallel.absolute_weight_source
    );
    assert!(parallel.absolute_length_source.is_some());
    assert_eq!(parallel.elements[0].kind, "contrast");
    assert_eq!(parallel.elements[0].length, Some(2.0));
    assert_eq!(
        result.data.rendering_options[0]
            .line_style_binding
            .as_ref()
            .unwrap()
            .status,
        Status::Complete
    );
}

#[test]
fn transformed_dependency_is_distinct_from_unitless_weight() {
    for (program, reason) in [
        (
            b"dash, transformed".as_slice(),
            "hsf.line_style_geometry_options",
        ),
        (
            b"1.5 weight, absolute, contrast, transformed".as_slice(),
            "hsf.line_style_weight_units",
        ),
    ] {
        let result = read(&definition(program), ParseMode::Partial).unwrap();
        let d = &result.data.line_styles[0];
        let p = d.program.as_ref().unwrap();
        assert_eq!(p.reason.as_deref(), Some(reason));
        assert_eq!(d.reason, p.reason);
        assert!(p.transformed_source.is_some());
        assert_eq!(result.data.placement_status, Status::Unsupported);
        assert!(read(&definition(program), ParseMode::Strict).is_err());
    }
    for program in [
        b"dash, transformed, dash".as_slice(),
        b"absolute, absolute weight, dash".as_slice(),
    ] {
        assert!(
            read(&definition(program), ParseMode::Partial)
                .unwrap()
                .data
                .line_styles[0]
                .program
                .is_none()
        );
    }
}
