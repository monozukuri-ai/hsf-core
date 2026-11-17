//! Records of third-party eDrawings publishers and of HSF 16.10: Window and
//! Window Pattern, images stored as uncompressed rows behind the 32-bit format
//! word, and the one-byte format field with JPEG payloads. All streams are
//! built here.
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
fn strict(body: &[u8], version: &str) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parsed_with(
        body,
        version,
        &ParseOptions {
            mode: ParseMode::Strict,
            ..Default::default()
        },
    )
}
fn images() -> ParseOptions {
    ParseOptions {
        image_profile: ImageProfile::U32RgbaJpegV1,
        ..Default::default()
    }
}
fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn line() -> Vec<u8> {
    [b"l".as_slice(), &floats(&[0., 0., 0., 1., 1., 1.])].concat()
}
fn window(area: [f32; 4]) -> Vec<u8> {
    [b"W".as_slice(), &floats(&area)].concat()
}
// An independently authored marker envelope of a 3 x 1 baseline JPEG.
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
/// A named 3 x 1 image; `wide` writes the format as a 32-bit word.
fn image(wide: bool, format: u32, compression: u8, payload: &[u8], alpha: &[u8]) -> Vec<u8> {
    let mut b = vec![b'i'];
    b.extend(floats(&[1., 2., 3.]));
    if wide {
        b.extend(format.to_le_bytes());
    } else {
        b.push(format as u8);
    }
    b.extend([1, b'n']);
    b.extend(3u32.to_le_bytes());
    b.extend(1u32.to_le_bytes());
    b.push(compression);
    if compression != 0 {
        b.extend((payload.len() as u32).to_le_bytes());
    }
    b.extend(payload);
    b.extend(alpha);
    b
}

#[test]
fn sixteen_ten_has_a_syntax_profile() {
    let p = strict(b"\0", "16.10").unwrap();
    assert_eq!(p.status, Status::Complete);
}

#[test]
fn window_and_window_pattern_are_framed_and_left_unevaluated() {
    let area = [-1., 1., -0.5, 0.25];
    let b = [window(area), b"p\x09".to_vec(), line(), vec![0]].concat();
    for version in ["7.00", "16.10", "19.10"] {
        let p = strict(&b, version).unwrap();
        assert_eq!(p.status, Status::Complete);
        let kinds: Vec<_> = p.data.records.iter().map(|r| &*r.kind).collect();
        assert_eq!(kinds, ["window", "window_pattern", "line", "termination"]);
        assert_eq!(
            p.data
                .source_bytes(p.data.records[0].payload.as_ref().unwrap())
                .unwrap(),
            floats(&area)
        );
        assert_eq!(p.data.records[0].source.length, 17);
        assert_eq!(p.data.records[1].value, Some(9));
        // The scene keeps both as settings whose effect it does not evaluate.
        let s = scene(
            &p,
            &SceneOptions {
                mode: ParseMode::Partial,
                ..Default::default()
            },
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        let settings: Vec<_> = s.data.rendering_options.iter().map(|o| &*o.kind).collect();
        assert_eq!(settings, ["window", "window_pattern"]);
        assert!(
            s.data
                .rendering_options
                .iter()
                .all(|o| o.interpretation_status == Status::Unsupported)
        );
        assert_eq!(
            s.diagnostics
                .iter()
                .filter(|d| d.code == "hsf.rendering_semantics")
                .count(),
            2
        );
        assert_eq!(s.data.primitives.len(), 1);
    }
    let cut = window(area);
    for end in 1..cut.len() {
        let e = strict(&cut[..end], "19.10").unwrap_err();
        assert_eq!(e.diagnostic.classification, Status::Invalid, "end={end}");
    }
    assert_eq!(
        strict(b"p", "19.10").unwrap_err().diagnostic.classification,
        Status::Invalid
    );
    let e = strict(&window([0., f32::NAN, 0., 1.]), "19.10").unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.nonfinite_window");
    // Whether 20.90 puts its precision byte here is not verified.
    for version in ["20.90", "21.15"] {
        let p = parsed_with(&b, version, &ParseOptions::default()).unwrap();
        assert!(p.data.records.is_empty(), "{version}");
        assert_eq!(p.diagnostics[0].code, "hsf.unsupported_window_precision");
    }
    assert!(build_info().capabilities.syntax_opcodes.contains(&b'W'));
    assert!(build_info().capabilities.syntax_opcodes.contains(&b'p'));
}

#[test]
fn images_are_stored_as_rows_or_as_jpeg_behind_either_format_field() {
    let rgb: Vec<u8> = (0..9).collect();
    let rgba: Vec<u8> = (0..12).collect();
    for (version, wide) in [("7.00", false), ("16.10", false), ("19.10", true)] {
        let prefix = if wide { "u32" } else { "byte" };
        for (format, compression, payload, alpha, kind, has_alpha) in [
            (0x82u32, 0u8, rgb.clone(), vec![], "rgb_v1", false),
            (0x83, 0, rgba.clone(), vec![], "rgba_v1", false),
            (0xa2, 2, jpeg(), vec![], "rgb_jpeg_v1", false),
            (0x87, 2, jpeg(), vec![], "jpeg_v1", false),
            (0xa3, 2, jpeg(), vec![0, 255], "rgba_jpeg_v1", true),
            (
                0x83,
                2,
                jpeg(),
                // a literal run of three alpha bytes
                vec![1, 4, 0, 0, 0, 3, 7, 8, 9],
                "rgba_jpeg_v1",
                true,
            ),
        ] {
            let record = image(wide, format, compression, &payload, &alpha);
            let b = [record.clone(), line(), vec![0]].concat();
            let p = parsed_with(&b, version, &images()).unwrap();
            let label = format!("{version} {format:#x} {compression}");
            assert!(p.unparsed.is_empty(), "{label}");
            let kinds: Vec<_> = p.data.records.iter().map(|r| &*r.kind).collect();
            assert_eq!(
                kinds,
                ["experimental_image", "line", "termination"],
                "{label}"
            );
            let i = p.data.records[0].image().unwrap();
            assert_eq!(i.profile, format!("{prefix}_{kind}"), "{label}");
            assert_eq!(i.format_options, format);
            assert_eq!((i.width, i.height, i.compression), (3, 1, compression));
            assert_eq!(p.data.source_bytes(&i.data).unwrap(), payload, "{label}");
            assert_eq!(i.alpha.is_some(), has_alpha, "{label}");
            assert_eq!(p.data.records[0].source.length, record.len() as u64);
            for end in 1..record.len() {
                let e = parsed_with(&record[..end], version, &images()).unwrap_err();
                assert_eq!(
                    e.diagnostic.classification,
                    Status::Invalid,
                    "{label} end={end}"
                );
            }
        }
        // Rows of the JPEG format, a run-length payload, BGRA, an unnamed
        // image and an unknown option stop at the opcode.
        for (format, compression) in [(0x87u32, 0u8), (0x82, 1), (0x84, 0), (0x02, 0), (0x92, 0)] {
            let record = image(wide, format, compression, &rgb, &[]);
            let p = parsed_with(&[line(), record, vec![0]].concat(), version, &images()).unwrap();
            assert_eq!(p.data.records.len(), 1, "{version} {format:#x}");
            assert_eq!(p.diagnostics[0].code, "hsf.image_profile");
        }
    }
    // A version without an observed image stops whatever the field holds.
    for version in ["8.05", "14.50", "17.85", "18.00", "21.15"] {
        let record = image(true, 0x82, 0, &rgb, &[]);
        let p = parsed_with(&[record, vec![0]].concat(), version, &images()).unwrap();
        assert!(p.data.records.is_empty(), "{version}");
        assert_eq!(p.diagnostics[0].code, "hsf.image_profile", "{version}");
    }
}
