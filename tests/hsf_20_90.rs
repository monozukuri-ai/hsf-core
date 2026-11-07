//! HSF 20.90, as written by SOLIDWORKS 2025 and 2026 eDrawings: one more byte
//! right after the opcode of bounds, cameras, views, modelling matrices,
//! shells, simple primitives, batches and images; segment references with a
//! leading zero and no trailing condition; and shell attribute 28. All streams
//! are built here.
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
const MATRIX: [f32; 12] = [1., 0., 0., 0., 1., 0., 0., 0., 1., 4., 5., 6.];
const CAMERA: [f32; 11] = [0., 0., 10., 0., 0., 0., 0., 1., 0., 2., 2.];
const PARAMETERS: [f32; 6] = [0., 0., 1., 0., 0., 1.];

/// A shell with three vertices, one compressed face and attribute 28
/// (width 2), laid out as 19.10 writes it after the 20.90 precision byte.
fn shell() -> Vec<u8> {
    let mut b = vec![b'S', 0, 0x1a, 0];
    b.extend(3u32.to_le_bytes());
    b.extend(floats(&[0., 0., 0., 1., 0., 0., 0., 1., 0.]));
    b.extend([1, 5, 0, 0, 0, 8, 3, 0, 1, 2]);
    b.extend([28, 2]);
    b.extend(floats(&PARAMETERS));
    b.push(0);
    b
}
fn reference(op: u8, path: &[u8]) -> Vec<u8> {
    [vec![op, 0, path.len() as u8], path.to_vec()].concat()
}
fn body() -> Vec<u8> {
    [
        [b"B\0\0".as_slice(), &floats(&[0., 0., 0., 1., 2., 3.])].concat(),
        [b"%\0".as_slice(), &floats(&MATRIX)].concat(),
        [b"}\0\0".as_slice(), &floats(&CAMERA), b"\x04view"].concat(),
        [b">\0\0".as_slice(), &floats(&CAMERA)].concat(),
        b"(\x04part".to_vec(),
        reference(b'<', b"/include library/shared/body0"),
        reference(b'{', b"/style"),
        [b"l\0".as_slice(), &floats(&[0., 0., 0., 1., 1., 1.])].concat(),
        [b"X\0".as_slice(), &floats(&[1., 2., 3.])].concat(),
        shell(),
        b")\0".to_vec(),
    ]
    .concat()
}

#[test]
fn precision_bytes_and_unconditional_references_frame_every_record() {
    let p = parsed(&body(), "20.90", ParseMode::Strict).unwrap();
    let kinds: Vec<_> = p.data.records.iter().map(|r| &*r.kind).collect();
    assert_eq!(
        kinds,
        [
            "bounding_info",
            "modelling_matrix",
            "view",
            "camera",
            "open_segment",
            "include_segment",
            "style_segment",
            "line",
            "marker",
            "shell",
            "close_segment",
            "termination",
        ]
    );
    let bytes = |s: &Option<SourceSpan>| p.data.source_bytes(s.as_ref().unwrap()).unwrap();
    // Payloads exclude the precision byte, so the scene reads them as before.
    assert_eq!(
        bytes(&p.data.records[0].payload),
        floats(&[0., 0., 0., 1., 2., 3.])
    );
    assert_eq!(bytes(&p.data.records[1].payload), floats(&MATRIX));
    let view = p.data.records[2].view().unwrap();
    assert_eq!(view.projection, 0);
    assert_eq!(p.data.source_bytes(&view.camera).unwrap(), floats(&CAMERA));
    assert_eq!(
        bytes(&p.data.records[5].payload),
        b"/include library/shared/body0"
    );
    assert!(p.data.records[5].condition.is_none());
    assert_eq!(bytes(&p.data.records[6].payload), b"/style");
    let shell = p.data.records[9].shell().unwrap();
    assert_eq!(shell.vertex_count, 3);
    assert_eq!(shell.attributes.len(), 1);
    assert_eq!(shell.attributes[0].opcode, 28);
    let parameters = shell.attributes[0]
        .fields
        .iter()
        .find(|f| f.name == "parameters")
        .unwrap();
    assert_eq!(
        p.data.source_bytes(&parameters.source).unwrap(),
        floats(&PARAMETERS)
    );
}

#[test]
fn the_same_bytes_are_not_valid_nineteen_ten() {
    // Without the version gate the extra byte would shift every operand.
    assert!(
        parsed(&body(), "19.10", ParseMode::Strict).is_err(),
        "20.90 framing must not be accepted under a 19.10 header"
    );
}

#[test]
fn nonzero_precision_or_reference_bytes_stop_without_guessing() {
    for (record, code) in [
        (
            [b"B\x01\0".as_slice(), &[0; 24]].concat(),
            "hsf.unsupported_bounds",
        ),
        (
            [b"%\x01".as_slice(), &[0; 48]].concat(),
            "hsf.unsupported_matrix_precision",
        ),
        (
            [b">\x01\0".as_slice(), &[0; 44]].concat(),
            "hsf.unsupported_camera_projection",
        ),
        (
            [b"l\x01".as_slice(), &[0; 24]].concat(),
            "hsf.unsupported_primitive_precision",
        ),
        (
            b"S\x01\x90\x02\0\0".to_vec(),
            "hsf.unsupported_shell_precision",
        ),
        (
            b"<\x01\x02ab".to_vec(),
            "hsf.unsupported_conditional_segment",
        ),
    ] {
        let p = parsed(&[record, vec![0]].concat(), "20.90", ParseMode::Partial).unwrap();
        assert!(p.data.records.is_empty());
        assert_eq!(p.diagnostics[0].code, code);
        assert_eq!(p.unparsed[0].offset, 15);
    }
}

#[test]
fn observed_visibility_bit26_is_accepted_for_twenty_ninety() {
    // Mask 0x0480_0081 with the fixed-width second extension, as in 19.10.
    let record = b"V\x81\x01\x00\x80\x00\x00\x04\x04".to_vec();
    let line = [b"l\0".as_slice(), &[0; 24]].concat();
    let p = parsed(
        &[record, line, vec![0]].concat(),
        "20.90",
        ParseMode::Strict,
    )
    .unwrap();
    assert_eq!(p.status, Status::Complete);
    assert_eq!(p.data.records[0].source.length, 9);
}

#[test]
fn shell_attribute_28_is_its_own_family() {
    // 27 and 28 may share a shell; two 28s may not.
    let mut b = shell();
    let end = b.len() - 1;
    let mut both = b[..end].to_vec();
    both.extend([27, 2]);
    both.extend(floats(&PARAMETERS));
    both.push(0);
    let p = parsed(&[both, vec![0]].concat(), "20.90", ParseMode::Strict).unwrap();
    let shell = p.data.records[0].shell().unwrap();
    assert_eq!(
        shell
            .attributes
            .iter()
            .map(|a| a.opcode)
            .collect::<Vec<_>>(),
        [28, 27]
    );
    b.truncate(end);
    b.extend([28, 2]);
    b.extend(floats(&PARAMETERS));
    b.push(0);
    let e = parsed(&[b, vec![0]].concat(), "20.90", ParseMode::Strict).unwrap_err();
    assert_eq!(e.diagnostic.code, "hsf.shell_attribute");
}

#[test]
fn every_cut_of_the_twenty_ninety_body_is_invalid() {
    let b = body();
    for end in 1..b.len() {
        for mode in [ParseMode::Strict, ParseMode::Partial] {
            let e = parsed(&b[..end], "20.90", mode).unwrap_err();
            assert_eq!(e.diagnostic.classification, Status::Invalid, "cut at {end}");
        }
    }
}
