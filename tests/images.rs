use hsf_core::*;

// An independently authored marker envelope, deliberately not a pixel decoder test.
fn image(alpha: &[u8]) -> Vec<u8> {
    let jpeg = [
        vec![
            255, 216, 255, 192, 0, 17, 8, 0, 1, 0, 3, 3, 1, 17, 0, 2, 17, 0, 3, 17, 0,
        ],
        vec![
            255, 218, 0, 12, 3, 1, 0, 2, 0, 3, 0, 0, 63, 0, 23, 255, 0, 42, 255, 217,
        ],
    ]
    .concat();
    let mut b = vec![b'i'];
    b.extend([0; 12]);
    b.extend(0xa3u32.to_le_bytes());
    b.extend([1, b'n']);
    b.extend(3u32.to_le_bytes());
    b.extend(1u32.to_le_bytes());
    b.push(2);
    b.extend((jpeg.len() as u32).to_le_bytes());
    b.extend(jpeg);
    b.extend(alpha);
    b
}
fn options() -> ParseOptions {
    ParseOptions {
        image_profile: ImageProfile::U32RgbaJpegV1,
        ..Default::default()
    }
}
fn run(b: &[u8], options: &ParseOptions) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        &[b";; HSF V19.10 \n".as_slice(), b].concat(),
        options,
        &mut Budget::new(Limits::default()),
    )
}
#[test]
fn image_is_opt_in_partial_and_preserves_owned_spans() {
    let b = [image(&[0, 255]), vec![0]].concat();
    let default = run(&b, &ParseOptions::default()).unwrap();
    assert!(default.data.records.is_empty());
    assert_eq!(default.diagnostics[0].code, "hsf.unknown_opcode");
    let p = run(&b, &options()).unwrap();
    assert_eq!(p.status, Status::Partial);
    assert!(p.unparsed.is_empty());
    assert_eq!(p.data.records.len(), 2);
    let i = p.data.records[0].image.as_ref().unwrap();
    assert_eq!((i.width, i.height), (3, 1));
    assert_eq!(p.data.source_bytes(&i.alpha).unwrap(), &[255]);
    assert_eq!(p.data.source_bytes(&i.name).unwrap(), b"n");
    let strict = ParseOptions {
        mode: ParseMode::Strict,
        ..options()
    };
    assert_eq!(
        run(&b, &strict).unwrap_err().diagnostic.code,
        "hsf.experimental_image"
    );
}
#[test]
fn image_cut_at_every_operand_byte_is_invalid_even_in_partial() {
    let b = image(&[1, 2, 0, 0, 0, 0, 255]);
    for mode in [ParseMode::Strict, ParseMode::Partial] {
        for end in 1..b.len() {
            let err = run(&b[..end], &ParseOptions { mode, ..options() }).unwrap_err();
            assert_eq!(err.diagnostic.classification, Status::Invalid, "end={end}");
            assert_eq!(err.diagnostic.opcode, Some(b'i'));
        }
    }
}
#[test]
fn alpha_zero_control_and_pixel_budget_are_exact_and_cumulative() {
    let b = [image(&[1, 2, 0, 0, 0, 0, 42]), vec![0]].concat();
    assert!(run(&b, &options()).is_ok());
    for alpha in [
        &[1, 2, 0, 0, 0, 255, 42][..],
        &[1, 2, 0, 0, 0, 1, 42][..],
        &[1, 1, 0, 0, 0, 0][..],
    ] {
        let bad = [image(alpha), vec![0]].concat();
        assert_eq!(
            run(&bad, &options()).unwrap_err().diagnostic.classification,
            Status::Invalid
        );
    }
    let two = [image(&[0, 0]), image(&[0, 255]), vec![0]].concat();
    let limited = ParseOptions {
        max_image_pixels: 5,
        ..options()
    };
    assert_eq!(
        run(&two, &limited).unwrap_err().diagnostic.code,
        "hsf.limit_image_pixels"
    );
    assert!(
        run(
            &two,
            &ParseOptions {
                max_image_pixels: 6,
                ..options()
            }
        )
        .is_ok()
    );
}
#[test]
fn unsupported_image_profiles_do_not_resynchronize() {
    for (at, value) in [(13, 0xb3), (14, 1), (27, 99)] {
        let mut b = image(&[0, 255]);
        b[at] = value;
        b.push(0);
        let p = run(&b, &options()).unwrap();
        assert_eq!(p.diagnostics[0].code, "hsf.image_profile");
        assert!(p.data.records.is_empty());
    }
}
#[test]
fn image_inventory_tags_the_image_without_previous_geometry() {
    let mut b = vec![b'X'];
    b.extend([0; 12]);
    b.extend(image(&[0, 255]));
    b.extend([b'q', b'r', 0, 0, 0, 0, 0]);
    let p = run(&b, &options()).unwrap();
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert_eq!(s.diagnostics.last().unwrap().code, "hsf.image_inventory");
    assert_eq!(s.data.references[0].target_id.as_deref(), Some("image:0"));
    assert_eq!(s.data.processed_records, 5);
    assert!(s.data.primitive_instances.is_empty());
}

fn texture(name: u8) -> Vec<u8> {
    vec![b't', 1, b't', 1, name, 0, 0]
}
fn inventory(body: &[u8], maximum: u64) -> Result<ParseOutcome<Scene>, HsfError> {
    let mut budget = Budget::new(Limits::default());
    let bytes = [b";; HSF V19.10 \n".as_slice(), body, &[0]].concat();
    let parsed = parse(&bytes, &options(), &mut budget)?;
    scene(
        &parsed,
        &SceneOptions {
            max_definitions: maximum,
            ..Default::default()
        },
        &mut budget,
    )
}
#[test]
fn image_name_matches_are_ordered_candidates_even_for_forward_and_duplicate_names() {
    for prefix in [true, false] {
        let mut b = Vec::new();
        if prefix {
            b.extend(texture(b'n'));
        }
        b.extend([b'(', 1, b'a']);
        b.extend(image(&[0, 255]));
        b.push(b')');
        b.extend([b'(', 1, b'b']);
        b.extend(image(&[0, 255]));
        b.push(b')');
        if !prefix {
            b.extend(texture(b'n'));
        }
        let s = inventory(&b, 3).unwrap();
        assert_eq!(
            s.data.textures[0].image_name_matches,
            ["image:0", "image:1"]
        );
        assert_eq!(s.data.images[0].node_id, "segment:1");
        assert_eq!(s.data.images[1].node_id, "segment:2");
        assert!(s.data.images[0].implicit_texture_local);
        assert_eq!(s.data.placement_status, Status::Unsupported);
        assert_eq!(s.data.images[0].position.value, [0.; 3]);
        assert!(s.data.instances.is_empty());
        assert_eq!(
            inventory(&b, 2).unwrap_err().diagnostic.code,
            "hsf.limit_definitions"
        );
    }
}
#[test]
fn image_candidates_use_exact_bytes_and_no_case_or_scope_lookup() {
    for name in [b'N', b'x'] {
        let b = [texture(name), image(&[0, 255])].concat();
        assert!(
            inventory(&b, 2).unwrap().data.textures[0]
                .image_name_matches
                .is_empty()
        );
    }
    let mut b = image(&[0, 255]);
    b[18] = 255;
    let s = inventory(&[texture(255), b].concat(), 2).unwrap();
    assert!(s.data.images[0].name.value.is_none());
    assert_eq!(s.data.textures[0].image_name_matches, ["image:0"]);
}
#[test]
fn image_resource_limits_are_shared_and_zero_rejects_images() {
    let b = image(&[0, 255]);
    assert_eq!(
        inventory(&b, 0).unwrap_err().diagnostic.code,
        "hsf.limit_definitions"
    );
    let body = [texture(b'n'), b].concat();
    assert_eq!(
        inventory(&body, 1).unwrap_err().diagnostic.code,
        "hsf.limit_definitions"
    );
}
#[test]
fn image_candidates_do_not_consume_images_after_semantic_stop() {
    let body = [
        texture(b'n'),
        image(&[0, 255]),
        vec![b'(', 2, b'.', b'.'],
        image(&[0, 255]),
        vec![b')'],
    ]
    .concat();
    let s = inventory(&body, 10).unwrap();
    assert_eq!(s.data.processed_records, 2);
    assert_eq!(s.data.images.len(), 1);
    assert_eq!(s.data.textures[0].image_name_matches, ["image:0"]);
    assert_eq!(s.diagnostics.last().unwrap().code, "hsf.segment_path");
}
