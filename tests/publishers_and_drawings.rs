//! Records observed in eDrawings drawings and third-party publishers: the 19.00
//! and 21.00 profiles, the transparency field of Rendering Options, clip
//! regions at 20.90, named JPEG images, quantized Mesh and batch points,
//! vertex colours, face patterns, ellipses and UTF-16 text code 8. All streams
//! are built here.
use hsf_core::*;

fn parsed(
    body: &[u8],
    version: &str,
    mode: ParseMode,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parsed_with(
        body,
        version,
        ParseOptions {
            mode,
            ..Default::default()
        },
    )
}
fn parsed_with(
    body: &[u8],
    version: &str,
    options: ParseOptions,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        &[format!(";; HSF V{version} \n").as_bytes(), body].concat(),
        &options,
        &mut Budget::new(Limits::default()),
    )
}
fn scene_of(p: &ParseOutcome<HsfDocument>) -> ParseOutcome<Scene> {
    scene(
        p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap()
}
fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn code(p: Result<ParseOutcome<HsfDocument>, HsfError>) -> String {
    match p {
        Ok(p) => p.diagnostics[0].code.clone(),
        Err(e) => e.diagnostic.code,
    }
}
fn line() -> Vec<u8> {
    [b"l".as_slice(), &floats(&[0., 0., 0., 1., 1., 1.])].concat()
}
/// 16-bit codes in little-endian 32-bit words, most significant first.
fn packed16(codes: &[u16]) -> Vec<u8> {
    let mut words = vec![0u32; (codes.len() * 16).div_ceil(32)];
    for (i, code) in codes.iter().enumerate() {
        words[i / 2] |= u32::from(*code) << if i % 2 == 0 { 16 } else { 0 };
    }
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

#[test]
fn catia_and_creo_10_versions_have_syntax_profiles() {
    for version in ["19.00", "21.00"] {
        let p = parsed(b"\0", version, ParseMode::Strict).unwrap();
        assert_eq!(p.status, Status::Complete);
    }
    // 21.00 is framed as 20.90: precision byte, and Visibility bit 26.
    let body = [
        b"l\0".to_vec(),
        floats(&[0., 0., 0., 1., 1., 1.]),
        b"V\xff\0\xff\xff\0\0\x07\0".to_vec(),
        vec![0],
    ]
    .concat();
    let p = parsed(&body, "21.00", ParseMode::Strict).unwrap();
    assert_eq!(p.data.records[0].kind, "line");
    assert_eq!(p.data.records[1].visibility().unwrap().mask, 0x07ffffff);
    // 19.00 has neither the precision byte nor the bit 26 allowance.
    assert_eq!(
        code(parsed(&body[26..], "19.00", ParseMode::Partial)),
        "hsf.visibility_mask"
    );
}

#[test]
fn transparency_follows_the_hidden_line_options() {
    // HSR byte (thsr 3), then transparency: blending with the extension byte
    // selecting nice z-sorting, as eDrawings drawings write it at 19.10.
    let mut b = vec![b'R'];
    for word in [0x80002000u32, 0x2002, 0x2000, 0x2002] {
        b.extend(word.to_le_bytes());
    }
    let record = [b.clone(), vec![0x30, 0x81, 0x02]].concat();
    let p = parsed(
        &[record.clone(), line(), vec![0]].concat(),
        "19.10",
        ParseMode::Strict,
    )
    .unwrap();
    let names: Vec<_> = p.data.records[0]
        .rendering()
        .unwrap()
        .fields
        .iter()
        .map(|f| &*f.name)
        .collect();
    assert_eq!(names, ["hsr", "transparency_options"]);
    assert_eq!(p.data.records[0].source.length, record.len() as u64);
    assert_eq!(p.data.records[1].kind, "line");
    // Depth peeling layers and minimum area follow their bits.
    let peeling = [b.clone(), vec![0x30, 0x31, 5], floats(&[2.5])].concat();
    let p = parsed(
        &[peeling.clone(), vec![0]].concat(),
        "19.10",
        ParseMode::Strict,
    )
    .unwrap();
    let fields = &p.data.records[0].rendering().unwrap().fields;
    assert_eq!(fields[2].name, "depth_peeling_layers");
    assert_eq!(fields[3].name, "depth_peeling_min_area");
    assert_eq!(p.data.records[0].source.length, peeling.len() as u64);
    // The style alone is older; depth peeling needs 13.10.
    let mut style = vec![b'R'];
    for word in [0x80000000u32, 2, 0, 2] {
        style.extend(word.to_le_bytes());
    }
    let p = parsed(
        &[style.clone(), vec![1, 0]].concat(),
        "8.05",
        ParseMode::Strict,
    )
    .unwrap();
    assert_eq!(p.data.records[0].rendering().unwrap().fields.len(), 1);
    for (body, version, expected) in [
        (
            [style.clone(), vec![0x11, 3]].concat(),
            "8.05",
            "hsf.rendering_options_version",
        ),
        (
            [b.clone(), vec![0x30, 0x30]].concat(),
            "8.05",
            "hsf.rendering_options_version",
        ),
        // Peeling algorithm and unknown extended bits have no verified operand.
        (
            [b.clone(), vec![0x30, 0x41]].concat(),
            "19.10",
            "hsf.transparency_options_mask",
        ),
        (
            [b.clone(), vec![0x30, 0x81, 0x04]].concat(),
            "19.10",
            "hsf.transparency_options_mask",
        ),
    ] {
        let body = [body, vec![0]].concat();
        assert_eq!(code(parsed(&body, version, ParseMode::Partial)), expected);
    }
    for end in 1..record.len() {
        assert_eq!(
            parsed(&record[..end], "19.10", ParseMode::Partial)
                .unwrap_err()
                .diagnostic
                .classification,
            Status::Invalid
        );
    }
}

#[test]
fn clip_regions_take_the_precision_byte_at_20_90() {
    let points = floats(&[0., 0., 0., 2., 0., 0., 2., 2., 0.]);
    let region = |prefix: &[u8]| [prefix, &3u32.to_le_bytes(), &points].concat();
    let body = [b"(\x01a".to_vec(), region(b"O\0\x05"), b")\0".to_vec()].concat();
    let p = parsed(&body, "20.90", ParseMode::Strict).unwrap();
    let clip = p.data.records[1].clip_region().unwrap();
    assert_eq!((clip.options, clip.point_count), (5, 3));
    assert_eq!(p.data.records[2].kind, "close_segment");
    // Option 5 also at 18.00 (no precision byte there), not at 14.50.
    let older = [region(b"O\x05"), vec![0]].concat();
    assert!(parsed(&older, "18.00", ParseMode::Strict).is_ok());
    assert_eq!(
        code(parsed(&older, "14.50", ParseMode::Partial)),
        "hsf.clip_region_options"
    );
    assert_eq!(
        code(parsed(
            &[region(b"O\x01\x05"), vec![0]].concat(),
            "20.90",
            ParseMode::Partial
        )),
        "hsf.unsupported_clip_region_precision"
    );
}

/// A baseline JPEG envelope of 3 x 1 pixels (markers only, not decodable).
fn jpeg() -> Vec<u8> {
    [
        vec![
            255, 216, 255, 192, 0, 17, 8, 0, 1, 0, 3, 3, 1, 17, 0, 2, 17, 0, 3, 17, 0,
        ],
        vec![
            255, 218, 0, 12, 3, 1, 0, 2, 0, 3, 0, 0, 63, 0, 23, 255, 0, 42, 255, 217,
        ],
    ]
    .concat()
}
fn image(flags: u32, alpha: &[u8]) -> Vec<u8> {
    let mut b = vec![b'i'];
    b.extend([0; 12]);
    b.extend(flags.to_le_bytes());
    b.extend([1, b'n']);
    b.extend(3u32.to_le_bytes());
    b.extend(1u32.to_le_bytes());
    b.push(2);
    let data = jpeg();
    b.extend((data.len() as u32).to_le_bytes());
    b.extend(data);
    b.extend(alpha);
    b
}

#[test]
fn named_jpeg_images_end_with_their_data() {
    let options = |mode| ParseOptions {
        image_profile: ImageProfile::U32RgbaJpegV1,
        mode,
        ..Default::default()
    };
    let body = [image(0x87, &[]), image(0xa3, &[0, 255]), vec![0]].concat();
    for version in ["19.00", "19.10"] {
        let p = parsed_with(&body, version, options(ParseMode::Partial)).unwrap();
        assert!(p.unparsed.is_empty());
        let named = p.data.records[0].image().unwrap();
        assert_eq!(named.profile, "u32_jpeg_v1");
        assert_eq!(
            (named.alpha_compression, named.alpha.as_ref()),
            (None, None)
        );
        let rgba = p.data.records[1].image().unwrap();
        assert_eq!(rgba.profile, "u32_rgba_jpeg_v1");
        assert_eq!(rgba.alpha_compression, Some(0));
        let s = scene_of(&p);
        assert_eq!(s.data.images[0].alpha_source, None);
        assert!(!s.data.images[0].implicit_texture_local);
    }
    for (body, version, expected) in [
        (image(0x87, &[]), "18.00", "hsf.image_profile"),
        (image(0x83, &[]), "19.10", "hsf.image_profile"),
    ] {
        let p = parsed_with(
            &[body, vec![0]].concat(),
            version,
            options(ParseMode::Partial),
        );
        assert_eq!(code(p), expected);
    }
}

/// A 2 x 2 Mesh with trivially quantized points (flag 1) spanning the box
/// (1, 2, 0)-(3, 6, 0), and quantized parameters (flag 8). Points beyond the
/// fourth repeat the first.
fn quantized_mesh(count: u32, scheme: u8) -> Vec<u8> {
    let mut b = vec![b'M', 9];
    b.extend(2u32.to_le_bytes());
    b.extend(2u32.to_le_bytes());
    b.push(scheme);
    b.extend(count.to_le_bytes());
    b.extend(floats(&[1., 2., 0., 3., 6., 0.]));
    b.push(16);
    let mut points = vec![
        0,
        0,
        0,
        u16::MAX,
        0,
        0,
        0,
        u16::MAX,
        0,
        u16::MAX,
        u16::MAX,
        0,
    ];
    points.resize(count as usize * 3, 0);
    let codes = packed16(&points);
    b.extend((codes.len() as u32).to_le_bytes());
    b.extend(codes);
    b.extend([3, 1, 3]);
    b.extend(floats(&[0., 0., 0., 1., 1., 0.]));
    b.push(8);
    b.extend(12u32.to_le_bytes());
    b.extend([0, 0, 0, 255, 0, 0, 0, 255, 0, 255, 255, 0]);
    b.push(0);
    b
}

#[test]
fn quantized_mesh_points_decode_within_their_box() {
    let body = [quantized_mesh(4, 1), vec![0]].concat();
    let p = parsed(&body, "19.00", ParseMode::Strict).unwrap();
    let m = p.data.records[0].mesh().unwrap();
    assert_eq!(m.point_quantization.as_ref().unwrap().bits_per_sample, 16);
    assert_eq!(m.vertices.length, 24);
    let s = scene_of(&p);
    assert_eq!(
        s.data.meshes[0].vertices,
        [[1., 2., 0.], [3., 2., 0.], [1., 6., 0.], [3., 6., 0.]]
    );
    assert_eq!(s.data.meshes[0].triangle_count, 2);
    assert_eq!(
        parsed(
            &[quantized_mesh(5, 1), vec![0]].concat(),
            "19.00",
            ParseMode::Partial
        )
        .unwrap_err()
        .diagnostic
        .code,
        "hsf.invalid_mesh_count"
    );
    assert_eq!(
        code(parsed(
            &[quantized_mesh(4, 2), vec![0]].concat(),
            "19.00",
            ParseMode::Partial
        )),
        "hsf.shell_point_compression"
    );
}

/// A compressed batch of two 2-point lines over X and Y (Z zero), optionally
/// with per-component X values in their own quantized block.
fn quantized_batch(flags: u16, values: bool, bits: u32, length_delta: i32) -> Vec<u8> {
    let mut b = vec![16];
    b.extend(flags.to_le_bytes());
    b.extend(4i32.to_le_bytes());
    b.extend(2i32.to_le_bytes());
    b.extend(2i32.to_le_bytes());
    b.push(1);
    let dims = if values { 1 } else { 2 };
    let codes = if values {
        packed16(&[0, u16::MAX, 0, u16::MAX])
    } else {
        packed16(&[0, 0, u16::MAX, 0, 0, u16::MAX, u16::MAX, u16::MAX])
    };
    b.extend(floats(if dims == 2 {
        &[0., 0., 4., 8.]
    } else {
        &[0., 8.]
    }));
    b.extend(bits.to_le_bytes());
    b.extend((codes.len() as i32 + length_delta).to_le_bytes());
    b.extend(codes);
    if values {
        b.push(1);
        b.extend(floats(&[10., 20.]));
        let values = packed16(&[0, u16::MAX]);
        b.extend((values.len() as u32).to_le_bytes());
        b.extend(values);
    }
    b
}

#[test]
fn quantized_batches_decode_points_and_one_axis_of_values() {
    let body = [quantized_batch(0x1001, false, 16, 0), vec![0]].concat();
    let p = parsed(&body, "19.00", ParseMode::Strict).unwrap();
    let l = p.data.records[0].poly_polyline().unwrap();
    assert_eq!(l.point_quantization.as_ref().unwrap().bits_per_sample, 16);
    assert!(l.leftover_quantization.is_none());
    let s = scene_of(&p);
    assert_eq!(
        s.data.primitives[0].points,
        [[0., 0., 0.], [4., 0., 0.], [0., 8., 0.], [4., 8., 0.]]
    );
    // Y unrestricted, X per component (quantized block), Z zero.
    let body = [quantized_batch(0x1003 & !2 | 0x300, true, 16, 0), vec![0]].concat();
    let p = parsed(&body, "19.00", ParseMode::Strict).unwrap();
    assert!(
        p.data.records[0]
            .poly_polyline()
            .unwrap()
            .leftover_quantization
            .is_some()
    );
    assert_eq!(
        scene_of(&p).data.primitives[0].points,
        [[10., 0., 0.], [10., 8., 0.], [20., 0., 0.], [20., 8., 0.]]
    );
    for (flags, values, bits, delta, expected) in [
        // Values of two axes, quantized points without an unrestricted axis,
        // unknown widths and the octet layout of three 8-bit axes stop.
        (0x3301u16, true, 16, 0, "hsf.poly_polyline_compression"),
        (0x1501, false, 16, 0, "hsf.poly_polyline_dimensions"),
        (0x1001, false, 32, 0, "hsf.poly_polyline_compression"),
        (0x1001, false, 16, 4, "hsf.invalid_poly_polyline"),
    ] {
        let body = [quantized_batch(flags, values, bits, delta), vec![0]].concat();
        assert_eq!(code(parsed(&body, "19.00", ParseMode::Partial)), expected);
    }
    let mut octets = vec![16];
    octets.extend(1u16.to_le_bytes());
    octets.extend(2i32.to_le_bytes());
    octets.extend(2i32.to_le_bytes());
    octets.push(1);
    octets.extend(floats(&[0.; 6]));
    octets.extend(8u32.to_le_bytes());
    assert_eq!(
        code(parsed(
            &[octets, vec![0]].concat(),
            "19.00",
            ParseMode::Partial
        )),
        "hsf.poly_polyline_compression"
    );
}

#[test]
fn uncompressed_batches_may_restrict_every_axis() {
    // Two dots: every axis is per component, so no unrestricted coordinates.
    let mut b = vec![16];
    b.extend(0x3f00u16.to_le_bytes());
    b.extend(4i32.to_le_bytes());
    b.extend(2i32.to_le_bytes());
    b.extend(2i32.to_le_bytes());
    b.extend(floats(&[1., 2., 3., 4., 5., 6.]));
    let p = parsed(&[b.clone(), vec![0]].concat(), "18.00", ParseMode::Strict).unwrap();
    let l = p.data.records[0].poly_polyline().unwrap();
    assert_eq!(
        (l.dimensions, l.points.length, l.leftovers.length),
        (0, 0, 24)
    );
    assert_eq!(
        scene_of(&p).data.primitives[0].points,
        [[1., 3., 5.], [1., 3., 5.], [2., 4., 6.], [2., 4., 6.]]
    );
}

/// A triangle Shell with the given optional attributes.
fn coloured_shell(attributes: &[u8]) -> Vec<u8> {
    let mut b = vec![b'S', 0x1a, 0];
    b.extend(3u32.to_le_bytes());
    b.extend(floats(&[0., 0., 0., 1., 0., 0., 0., 1., 0.]));
    b.extend([1, 5, 0, 0, 0, 8, 3, 0, 1, 2]);
    b.extend(attributes);
    b.push(0);
    b
}
fn colours(opcode: u8, bits: u8, length: u32) -> Vec<u8> {
    [
        vec![opcode, 1, bits],
        length.to_le_bytes().to_vec(),
        vec![7; 9],
    ]
    .concat()
}

#[test]
fn vertex_colours_are_framed_for_faces_edges_and_markers() {
    let attributes = [colours(5, 8, 9), colours(7, 8, 9), colours(9, 8, 9)].concat();
    let p = parsed(
        &[coloured_shell(&attributes), vec![0]].concat(),
        "18.00",
        ParseMode::Strict,
    )
    .unwrap();
    let s = p.data.records[0].shell().unwrap();
    assert_eq!(
        s.attributes.iter().map(|a| a.opcode).collect::<Vec<_>>(),
        [5, 7, 9]
    );
    assert_eq!(s.attributes[0].fields.last().unwrap().name, "colors");
    let scene = scene_of(&p);
    assert_eq!(scene.data.meshes[0].triangle_count, 1);
    assert!(
        scene
            .diagnostics
            .iter()
            .any(|d| d.code == "hsf.mesh_attributes")
    );
    for (attributes, expected) in [
        (colours(5, 16, 9), "hsf.shell_attribute_compression"),
        (
            [colours(5, 8, 9), colours(5, 8, 9)].concat(),
            "hsf.shell_attribute",
        ),
        (colours(11, 8, 9), "hsf.shell_attribute"),
    ] {
        let body = [coloured_shell(&attributes), vec![0]].concat();
        assert_eq!(code(parsed(&body, "18.00", ParseMode::Partial)), expected);
    }
    let body = [coloured_shell(&colours(7, 8, 8)), vec![0]].concat();
    assert_eq!(
        parsed(&body, "18.00", ParseMode::Partial)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.invalid_shell_attribute_length"
    );
}

#[test]
fn face_patterns_are_one_enumerated_byte() {
    let p = parsed(
        &[b"P\x08".to_vec(), line(), vec![0]].concat(),
        "19.00",
        ParseMode::Strict,
    )
    .unwrap();
    assert_eq!(p.data.records[0].kind, "face_pattern");
    assert_eq!(p.data.records[0].value, Some(8));
    assert_eq!(p.data.records[1].kind, "line");
    let s = scene_of(&p);
    assert!(
        s.diagnostics
            .iter()
            .any(|d| d.code == "hsf.rendering_semantics")
    );
    assert_eq!(
        code(parsed(b"P\xff\0", "19.00", ParseMode::Partial)),
        "hsf.face_pattern"
    );
}

#[test]
fn ellipses_keep_their_axis_points_and_parametric_limits() {
    let points = floats(&[5., 5., 0., 9., 5., 0., 5., 7., 0.]);
    let body = [
        b"E".to_vec(),
        points.clone(),
        b"e".to_vec(),
        points.clone(),
        floats(&[0.25, 0.75]),
        vec![0],
    ]
    .concat();
    let p = parsed(&body, "19.10", ParseMode::Strict).unwrap();
    assert_eq!(p.data.records[0].kind, "ellipse");
    assert_eq!(p.data.records[1].kind, "elliptical_arc");
    let s = scene_of(&p);
    let [ellipse, arc] = &s.data.primitives[..] else {
        panic!("two primitives");
    };
    assert_eq!(ellipse.points, [[5., 5., 0.], [9., 5., 0.], [5., 7., 0.]]);
    assert!(ellipse.limits.is_none());
    assert_eq!(arc.limits.as_ref().unwrap().value, [0.25, 0.75]);
    assert_eq!(s.data.primitive_instances.len(), 2);
    // Unobserved at 20.90 and later; non-finite values are invalid.
    assert_eq!(
        code(parsed(&body, "20.90", ParseMode::Partial)),
        "hsf.unsupported_primitive_precision"
    );
    let bad = [b"e".to_vec(), points, floats(&[f32::NAN, 1.]), vec![0]].concat();
    assert_eq!(
        parsed(&bad, "19.10", ParseMode::Partial)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.nonfinite_ellipse"
    );
}

#[test]
fn text_code_8_is_little_endian_utf16() {
    let label = "Note 寸法"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let text = |bytes: &[u8]| {
        [
            b"x".as_slice(),
            &floats(&[1., 2., 3.]),
            &[8, bytes.len() as u8],
            bytes,
            &[0, 0],
        ]
        .concat()
    };
    let p = parsed(&text(&label), "18.00", ParseMode::Strict).unwrap();
    let s = scene_of(&p);
    let t = s.data.primitives[0].text.as_ref().unwrap();
    assert_eq!(
        (t.encoding.as_str(), t.value.as_deref()),
        ("utf_16_le", Some("Note 寸法"))
    );
    assert_eq!(
        scene(
            &parsed(&text(b"x"), "18.00", ParseMode::Strict).unwrap(),
            &SceneOptions::default(),
            &mut Budget::new(Limits::default()),
        )
        .unwrap_err()
        .diagnostic
        .code,
        "hsf.invalid_text_encoding"
    );
}
