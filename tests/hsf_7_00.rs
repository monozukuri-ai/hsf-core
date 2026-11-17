//! HSF 7.00, as early eDrawings writers produced it: the callback record,
//! the published one-byte image format with uncompressed pixel rows, and
//! Shell normals coded as indices into the published table of unit vectors.
//! All streams are built here.
use hsf_core::*;

fn parsed_with(
    body: &[u8],
    version: &str,
    options: &ParseOptions,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        &[format!(";; HSF V{version} \n").as_bytes(), body].concat(),
        options,
        &mut Budget::new(Limits::default()),
    )
}
fn parsed(
    body: &[u8],
    version: &str,
    mode: ParseMode,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parsed_with(
        body,
        version,
        &ParseOptions {
            mode,
            ..Default::default()
        },
    )
}
fn images(mode: ParseMode) -> ParseOptions {
    ParseOptions {
        image_profile: ImageProfile::U32RgbaJpegV1,
        mode,
        ..Default::default()
    }
}
fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn line() -> Vec<u8> {
    [b"l".as_slice(), &floats(&[0., 0., 0., 1., 1., 1.])].concat()
}
fn callback(text: &[u8]) -> Vec<u8> {
    let mut b = vec![7];
    b.extend((text.len() as u16).to_le_bytes());
    b.extend(text);
    b
}
/// A named image of `width` by `height` pixels with `channels` bytes each.
fn image(format: u8, width: u32, height: u32, channels: usize, compression: u8) -> Vec<u8> {
    let mut b = vec![b'i'];
    b.extend(floats(&[1., 2., 3.]));
    b.extend([format, 2, b'i', b'm']);
    b.extend(width.to_le_bytes());
    b.extend(height.to_le_bytes());
    b.push(compression);
    b.extend((0..width as usize * height as usize * channels).map(|i| i as u8));
    b
}
/// A first-pass Shell of three points and one triangle whose normals are
/// `bits`-wide table codes in `payload`.
fn shell(scheme: u8, bits: u8, declared: i32, payload: &[u8]) -> Vec<u8> {
    let mut b = vec![b'S', 26, 0];
    b.extend(3u32.to_le_bytes());
    b.extend(floats(&[0., 0., 0., 1., 0., 0., 0., 1., 0.]));
    b.extend([1, 5, 0, 0, 0, 8, 3, 0, 1, 2]);
    b.extend([1, scheme, bits]);
    b.extend(declared.to_le_bytes());
    b.extend(payload);
    b.push(0);
    b
}

#[test]
fn seven_zero_has_a_syntax_profile() {
    let p = parsed(b"\0", "7.00", ParseMode::Strict).unwrap();
    assert_eq!(p.status, Status::Complete);
    assert!(
        build_info()
            .capabilities
            .syntax_versions
            .contains(&"7.00".to_string())
    );
    assert!(build_info().capabilities.syntax_opcodes.contains(&7));
}

#[test]
fn callbacks_are_framed_and_left_unevaluated() {
    let text = b"draw segment = edrawings_draw";
    let b = [callback(text), callback(b""), line(), vec![0]].concat();
    for version in ["7.00", "19.10"] {
        let p = parsed(&b, version, ParseMode::Strict).unwrap();
        assert_eq!(p.status, Status::Complete);
        let kinds: Vec<_> = p.data.records.iter().map(|r| &*r.kind).collect();
        assert_eq!(kinds, ["callback", "callback", "line", "termination"]);
        let r = &p.data.records[0];
        assert_eq!(r.source.length, 3 + text.len() as u64);
        assert_eq!(
            p.data.source_bytes(r.payload.as_ref().unwrap()).unwrap(),
            text
        );
        assert_eq!(p.data.records[1].payload.as_ref().unwrap().length, 0);
        // The scene takes nothing from a callback and still draws what follows.
        let s = scene(
            &p,
            &SceneOptions::default(),
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        assert_eq!(s.data.primitives.len(), 1);
    }
    let cut = callback(text);
    for end in 1..cut.len() {
        let e = parsed(&cut[..end], "7.00", ParseMode::Partial).unwrap_err();
        assert_eq!(e.diagnostic.classification, Status::Invalid, "end={end}");
        assert_eq!(e.diagnostic.opcode, Some(7));
    }
    // The string limit applies as it does to other strings.
    let e = parsed_with(
        &callback(text),
        "7.00",
        &ParseOptions {
            max_string_bytes: 4,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_string");
}

#[test]
fn byte_format_images_are_framed_under_the_image_profile() {
    for (format, channels, profile) in [(0x82, 3, "byte_rgb_v1"), (0x83, 4, "byte_rgba_v1")] {
        let record = image(format, 3, 2, channels, 0);
        let b = [record.clone(), line(), vec![0]].concat();
        // Images stay opt-in: the default stops at the opcode.
        let default = parsed(&b, "7.00", ParseMode::Partial).unwrap();
        assert!(default.data.records.is_empty());
        assert_eq!(default.diagnostics[0].code, "hsf.unknown_opcode");
        let p = parsed_with(&b, "7.00", &images(ParseMode::Partial)).unwrap();
        assert_eq!(p.status, Status::Partial);
        assert!(p.unparsed.is_empty());
        let kinds: Vec<_> = p.data.records.iter().map(|r| &*r.kind).collect();
        assert_eq!(kinds, ["experimental_image", "line", "termination"]);
        assert_eq!(p.diagnostics.len(), 1);
        assert_eq!(p.diagnostics[0].code, "hsf.experimental_image");
        assert_eq!(p.diagnostics[0].severity, "warning");
        let i = p.data.records[0].image().unwrap();
        assert_eq!(i.profile, profile);
        assert_eq!(i.format_options, u32::from(format));
        assert_eq!((i.width, i.height, i.compression), (3, 2, 0));
        assert_eq!(p.data.source_bytes(&i.name).unwrap(), b"im");
        assert_eq!(
            p.data.source_bytes(&i.position).unwrap(),
            floats(&[1., 2., 3.])
        );
        let pixels: Vec<u8> = (0..6 * channels).map(|i| i as u8).collect();
        assert_eq!(p.data.source_bytes(&i.data).unwrap(), pixels);
        assert!(i.alpha.is_none() && i.alpha_compression.is_none());
        assert_eq!(p.data.records[0].source.length, record.len() as u64);
        // The scene keeps the image as an owned definition and goes on.
        let s = scene(
            &p,
            &SceneOptions::default(),
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        assert_eq!(s.data.images.len(), 1);
        assert_eq!(s.data.images[0].profile, profile);
        assert_eq!(s.data.images[0].data_source.length, pixels.len() as u64);
        assert_eq!(s.data.primitives.len(), 1);
        // Strict parsing refuses the profile after the frame was checked.
        let e = parsed_with(&b, "7.00", &images(ParseMode::Strict)).unwrap_err();
        assert_eq!(e.diagnostic.code, "hsf.experimental_image");
        for end in 1..record.len() {
            let e = parsed_with(&record[..end], "7.00", &images(ParseMode::Partial)).unwrap_err();
            assert_eq!(e.diagnostic.classification, Status::Invalid, "end={end}");
            assert_eq!(e.diagnostic.opcode, Some(b'i'));
        }
    }
}

#[test]
fn other_byte_format_images_stop_at_the_opcode() {
    for (record, code) in [
        // Unnamed RGB, mapped and BGRA pixels, an explicit size option.
        (image(0x02, 1, 1, 3, 0), "hsf.image_profile"),
        (image(0x80, 1, 1, 1, 0), "hsf.image_profile"),
        (image(0x84, 1, 1, 4, 0), "hsf.image_profile"),
        (image(0x92, 1, 1, 3, 0), "hsf.image_profile"),
        // A run-length payload, and uncompressed rows of the JPEG format.
        (image(0x82, 1, 1, 3, 1), "hsf.image_profile"),
        (image(0x87, 1, 1, 3, 0), "hsf.image_profile"),
    ] {
        let p = parsed_with(
            &[line(), record, vec![0]].concat(),
            "7.00",
            &images(ParseMode::Partial),
        )
        .unwrap();
        assert_eq!(p.status, Status::Partial);
        assert_eq!(p.data.records.len(), 1);
        assert_eq!(p.diagnostics[0].code, code);
        assert_eq!(p.diagnostics[0].opcode, Some(b'i'));
        assert_eq!(p.unparsed.len(), 1);
    }
    let mut empty = image(0x82, 1, 1, 3, 0);
    empty[14] = 0;
    let e = parsed_with(&empty, "7.00", &images(ParseMode::Partial)).unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.invalid_image");
    let e = parsed_with(
        &image(0x82, 0, 1, 3, 0),
        "7.00",
        &images(ParseMode::Partial),
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.invalid_image");
    // The pixel budget is charged before the rows are taken.
    let e = parsed_with(
        &[image(0x82, 3, 2, 3, 0), vec![0]].concat(),
        "7.00",
        &ParseOptions {
            max_image_pixels: 5,
            ..images(ParseMode::Partial)
        },
    )
    .unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.limit_image_pixels");
    // Other versions keep their own image framing.
    for version in ["8.05", "19.10"] {
        let p = parsed_with(
            &[image(0x82, 3, 2, 3, 0), vec![0]].concat(),
            version,
            &images(ParseMode::Partial),
        )
        .unwrap();
        assert!(p.data.records.is_empty(), "{version}");
        assert_eq!(p.diagnostics[0].code, "hsf.image_profile", "{version}");
    }
}

#[test]
fn table_coded_normals_are_framed_and_kept_as_codes() {
    // Three 10-bit codes take 30 bits: four bytes, not a 32-bit word multiple
    // of the three-component layouts. Three 8-bit codes take three bytes.
    for (bits, payload) in [(10u8, vec![0x12, 0x34, 0x56, 0x78]), (8, vec![1, 2, 3])] {
        let b = [
            shell(3, bits, payload.len() as i32, &payload),
            line(),
            vec![0],
        ]
        .concat();
        let p = parsed(&b, "7.00", ParseMode::Strict).unwrap();
        assert_eq!(p.status, Status::Complete);
        let kinds: Vec<_> = p.data.records.iter().map(|r| &*r.kind).collect();
        assert_eq!(kinds, ["shell", "line", "termination"]);
        let layout = p.data.records[0].shell().unwrap();
        assert_eq!(layout.attributes.len(), 1);
        let a = &layout.attributes[0];
        assert_eq!(a.opcode, 1);
        let field = |name: &str| a.fields.iter().find(|f| f.name == name).unwrap();
        assert_eq!(
            p.data.source_bytes(&field("compression").source).unwrap(),
            [3]
        );
        assert_eq!(
            p.data
                .source_bytes(&field("bits_per_sample").source)
                .unwrap(),
            [bits]
        );
        assert_eq!(field("normals").encoding, "packed_table_codes");
        assert_eq!(
            p.data.source_bytes(&field("normals").source).unwrap(),
            payload
        );
        // The mesh keeps its points and faces; the codes stay as stored.
        let s = scene(
            &p,
            &SceneOptions::default(),
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        let m = &s.data.meshes[0];
        assert_eq!((m.vertex_count, m.triangle_count), (3, 1));
        assert!(m.normals.is_empty());
        assert_eq!(m.normal_count, 0);
        let n = m.vertex_normals.as_ref().unwrap();
        assert_eq!(n.encoding, "table_codes");
        assert_eq!((n.compression, n.bits_per_sample), (Some(3), Some(bits)));
        assert_eq!(n.values_source.length, payload.len() as u64);
        assert!(n.bounds.is_empty());
        assert_eq!(s.data.primitives.len(), 1);
    }
}

#[test]
fn table_coded_normals_check_width_and_length() {
    // The table exists for 8 and 10 bits only.
    for bits in [2u8, 9, 12, 31] {
        let p = parsed(&shell(3, bits, 4, &[0; 4]), "7.00", ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty(), "bits={bits}");
        assert_eq!(p.diagnostics[0].code, "hsf.shell_attribute_bits");
    }
    // The declared length must be the byte-packed size of the codes.
    for declared in [3, 5, 8] {
        let e = parsed(&shell(3, 10, declared, &[0; 8]), "7.00", ParseMode::Partial).unwrap_err();
        assert_eq!(e.diagnostic.code, "hsf.invalid_shell_attribute_length");
    }
    let record = shell(3, 10, 4, &[0; 4]);
    for end in 1..record.len() {
        let e = parsed(&record[..end], "7.00", ParseMode::Partial).unwrap_err();
        assert_eq!(e.diagnostic.classification, Status::Invalid, "end={end}");
    }
    // Unknown schemes still stop at the Shell.
    for scheme in [2u8, 4, 5, 6] {
        let p = parsed(&shell(scheme, 10, 4, &[0; 4]), "7.00", ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty(), "scheme={scheme}");
        assert_eq!(p.diagnostics[0].code, "hsf.shell_attribute_compression");
    }
    // So do table codes in other versions: their packing is not verified there.
    for version in ["8.05", "14.50", "19.10"] {
        let p = parsed(&shell(3, 10, 4, &[0; 4]), version, ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty(), "{version}");
        assert_eq!(p.diagnostics[0].code, "hsf.shell_attribute_compression");
    }
}
