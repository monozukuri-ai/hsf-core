//! Records observed in SOLIDWORKS 2011 and 2012 eDrawings (HSF 17.85 and 18.00),
//! a 2025 SOLIDWORKS writer at 19.10, and the Creo 11 eDrawings publisher (HSF
//! 21.15): selectability, simple reflection and quantized Shell points. All
//! streams are built here.
use hsf_core::*;

fn parsed(
    body: &[u8],
    version: &str,
    mode: ParseMode,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        &[format!(";; HSF V{version} \n").as_bytes(), body].concat(),
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}
fn render(high: u32, payload: &[u8]) -> Vec<u8> {
    let mut b = vec![b'R'];
    for word in [0x80000000u32, high, 0, high] {
        b.extend(word.to_le_bytes());
    }
    b.extend(payload);
    b
}
/// 16-bit codes packed into little-endian 32-bit words, most significant first.
fn packed(codes: &[u16]) -> Vec<u8> {
    let mut words = vec![0u32; (codes.len() * 16).div_ceil(32)];
    for (i, code) in codes.iter().enumerate() {
        words[i / 2] |= u32::from(*code) << if i % 2 == 0 { 16 } else { 0 };
    }
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}
/// A first-pass Shell with three quantized points spanning (0,0,0)-(2,4,8) and
/// one triangle.
fn quantized_shell(scheme: u8, bits: u8, length_delta: i32, bounds: [f32; 6]) -> Vec<u8> {
    let codes = packed(&[0, 0, 0, u16::MAX, 0, 0, 0, u16::MAX, u16::MAX]);
    let mut b = vec![b'S', 0x13, 0, scheme];
    b.extend(3u32.to_le_bytes());
    b.extend(floats(&bounds));
    b.push(bits);
    b.extend((codes.len() as i32 + length_delta).to_le_bytes());
    b.extend(codes);
    b.extend([1, 5, 0, 0, 0, 8, 3, 0, 1, 2]);
    b
}
const BOUNDS: [f32; 6] = [0., 0., 0., 2., 4., 8.];

#[test]
fn eleven_twelve_and_creo_versions_have_syntax_profiles() {
    for version in ["17.85", "18.00", "21.15"] {
        let p = parsed(b"\0", version, ParseMode::Strict).unwrap();
        assert_eq!(p.status, Status::Complete);
    }
}

#[test]
fn selectability_is_framed_in_both_widths() {
    let b = [
        b"!\x07\x01\x02\x03\x04\x05".to_vec(),
        [b"!\xff\x43".as_slice(), &[0; 10]].concat(),
        [b"l".as_slice(), &floats(&[0., 0., 0., 1., 1., 1.])].concat(),
        vec![0],
    ]
    .concat();
    let p = parsed(&b, "19.10", ParseMode::Strict).unwrap();
    let r = &p.data.records;
    assert_eq!(r[0].kind, "selectability");
    assert_eq!(r[0].value, Some(7));
    assert_eq!(r[0].source.length, 7);
    assert_eq!(r[1].value, Some(0x43ff));
    assert_eq!(r[1].source.length, 13);
    assert_eq!(r[2].kind, "line");
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert_eq!(s.data.primitives.len(), 1);
}

#[test]
fn simple_reflection_operands_follow_its_flags() {
    // Off, without operands, as SOLIDWORKS writes it.
    let p = parsed(
        &[
            render(0x80000, &[2]),
            b"l".to_vec(),
            floats(&[0.; 6]),
            vec![0],
        ]
        .concat(),
        "19.10",
        ParseMode::Strict,
    )
    .unwrap();
    let fields = &p.data.records[0].rendering.as_ref().unwrap().fields;
    assert_eq!(fields.last().unwrap().name, "simple_reflection_options");
    assert_eq!(p.data.records[1].kind, "line");
    // Plane, opacity, blur and attenuation: 0x4c with the extension byte, then 0x100.
    let operands = [&[0xccu8, 0x01][..], &[0; 32]].concat();
    let b = render(0x80000, &operands);
    let p = parsed(&[b.clone(), vec![0]].concat(), "19.10", ParseMode::Strict).unwrap();
    assert_eq!(p.data.records[0].source.length, b.len() as u64);
    let names: Vec<_> = p.data.records[0]
        .rendering
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert!(names.ends_with(&[
        "simple_reflection_options",
        "reflection_plane",
        "reflection_opacity",
        "reflection_blur",
        "reflection_attenuation",
    ]));
    assert_eq!(
        parsed(&b, "14.50", ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.rendering_options_version"
    );
    assert_eq!(
        parsed(&render(0x80000, &[0x80, 0x02]), "19.10", ParseMode::Strict)
            .unwrap_err()
            .diagnostic
            .code,
        "hsf.simple_reflection_mask"
    );
}

#[test]
fn quantized_points_decode_across_their_box() {
    let p = parsed(
        &[quantized_shell(1, 16, 0, BOUNDS), vec![0]].concat(),
        "19.10",
        ParseMode::Strict,
    )
    .unwrap();
    let shell = p.data.records[0].shell.as_ref().unwrap();
    assert_eq!(shell.vertex_count, 3);
    let q = shell.point_quantization.as_ref().unwrap();
    assert_eq!((q.scheme, q.bits_per_sample), (1, 16));
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert_eq!(
        s.data.meshes[0].vertices,
        [[0., 0., 0.], [2., 0., 0.], [0., 4., 8.]]
    );
    assert_eq!(s.data.meshes[0].triangles.len(), 1);
}

#[test]
fn quantized_point_profiles_outside_the_observed_one_stop() {
    for (b, code) in [
        (
            quantized_shell(2, 16, 0, BOUNDS),
            "hsf.shell_point_compression",
        ),
        (quantized_shell(1, 1, 0, BOUNDS), "hsf.shell_point_bits"),
        (
            quantized_shell(1, 16, 4, BOUNDS),
            "hsf.invalid_shell_point_length",
        ),
        (
            quantized_shell(1, 16, 0, [3., 0., 0., 2., 4., 8.]),
            "hsf.invalid_shell_point_bounds",
        ),
    ] {
        let e = parsed(&[b, vec![0]].concat(), "19.10", ParseMode::Strict).unwrap_err();
        assert_eq!(e.diagnostic.code, code);
    }
}

#[test]
fn creo_visibility_bit26_is_accepted() {
    // Mask 0x0480_0081 with the fixed-width second extension.
    let record = b"V\x81\x01\x00\x80\x00\x00\x04\x04".to_vec();
    let line = [b"l\0".as_slice(), &[0; 24]].concat();
    let p = parsed(
        &[record, line, vec![0]].concat(),
        "21.15",
        ParseMode::Strict,
    )
    .unwrap();
    assert_eq!(p.status, Status::Complete);
}
