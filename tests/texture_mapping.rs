use hsf_core::*;
fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn matrix(x: f32) -> Vec<u8> {
    [
        vec![b'$'],
        floats(&[x, 0., 0., 0., -3., 0., 0., 0., 1., 5., 7., 0.]),
    ]
    .concat()
}
fn grid(width: u8) -> Vec<u8> {
    let mut b = vec![b'M', 8];
    b.extend(2i32.to_le_bytes());
    b.extend(2i32.to_le_bytes());
    b.extend(floats(&[0., 0., 0., 1., 0., 0., 0., 1., 0., 1., 1., 0.]));
    b.extend([27, width]);
    b.extend(floats(
        &(0..4 * u16::from(width))
            .map(|n| f32::from(n) / 4. - 1.)
            .collect::<Vec<_>>(),
    ));
    b.push(0);
    b
}
fn parsed(body: &[u8]) -> ParseOutcome<HsfDocument> {
    parse(
        &[b";; HSF V19.10 \n", body, b"\0"].concat(),
        &ParseOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap()
}
fn read(body: &[u8], mode: ParseMode) -> Result<ParseOutcome<Scene>, HsfError> {
    scene(
        &parsed(body),
        &SceneOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
#[test]
fn texture_matrix_is_an_overriding_attribute_not_a_modelling_transform() {
    let b = [
        matrix(10.),
        b"(\x04defs".to_vec(),
        matrix(30.),
        b"){\x05/defs".to_vec(),
        matrix(2.),
        grid(2),
    ]
    .concat();
    let p = parsed(&b);
    let s = scene(
        &p,
        &SceneOptions {
            mode: ParseMode::Strict,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    let i = &s.data.instances[0];
    let a = i.display.texture_matrix.as_ref().unwrap();
    assert_eq!(
        a.value,
        [
            2., 0., 0., 0., 0., -3., 0., 0., 0., 0., 1., 0., 5., 7., 0., 1.
        ]
    );
    assert_eq!(p.data.source_bytes(&a.source), Some(matrix(2.).as_slice()));
    assert_eq!(i.world_transform, IDENTITY);
    assert_eq!(s.status, Status::Complete);
    assert!(
        s.data
            .rendering_options
            .iter()
            .all(|o| o.interpretation_status == Status::Complete)
    );
}
#[test]
fn all_parameter_widths_preserve_values_spans_and_vertex_alignment() {
    for w in [1, 2, 3, 4, 5, 128] {
        let p = parsed(&grid(w));
        let s = scene(
            &p,
            &SceneOptions::default(),
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        let m = &s.data.meshes[0];
        let a = m.vertex_parameters.as_ref().unwrap();
        assert_eq!((a.width, a.count), (w, 4));
        assert_eq!(
            m.parameters,
            (0..4 * u16::from(w))
                .map(|n| f32::from(n) / 4. - 1.)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            p.data.source_bytes(&a.values_source),
            Some(floats(&m.parameters).as_slice())
        );
        assert_eq!(m.triangles, [[0, 2, 1], [3, 1, 2]]);
        assert_eq!(s.status, Status::Complete);
    }
}
#[test]
fn matrix_styles_keep_conflicts_cycles_and_limits_closed() {
    let b = [
        b"(\x01a".to_vec(),
        matrix(2.),
        b")(\x01b".to_vec(),
        matrix(2.),
        b"){\x02/a{\x02/b".to_vec(),
        grid(2),
    ]
    .concat();
    let s = read(&b, ParseMode::Partial).unwrap();
    assert_eq!(
        s.data.style_resolutions[0].reason.as_deref(),
        Some("hsf.style_conflict")
    );
    assert!(s.data.instances.is_empty());
    assert!(read(&b, ParseMode::Strict).is_err());
    let p = parsed(&[matrix(2.), grid(2)].concat());
    let e = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits {
            max_output_bytes: 1000,
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.classification, Status::LimitExceeded);
}
#[test]
fn stored_options_do_not_imply_material_evaluation_or_extension_defaults() {
    let b = b"t\x01a\x01b\x01\0\x01t\x01c\x01d\x01\x80\x02\0\x01\x01t\x01e\x01f\x01\0\xff";
    let s = read(b, ParseMode::Partial).unwrap();
    let a = &s.data.textures[0];
    assert_eq!(a.options.enumerations[0].value.as_deref(), Some("uv"));
    assert_eq!(a.options.decoding_status, Status::Complete);
    assert_eq!(a.interpretation_status, Status::Unsupported);
    let b = &s.data.textures[1].options;
    assert_eq!(b.unevaluated_flags, 0x20000);
    assert_eq!(b.enumerations[1].raw, 1);
    assert!(b.enumerations[1].value.is_none());
    assert!(s.data.textures[2].options.enumerations[0].value.is_none());
    assert!(s.data.instances.is_empty());
}
