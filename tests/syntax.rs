use flate2::{Compression, write::ZlibEncoder};
use hsf_core::*;
use std::io::Write;

fn file(body: &[u8]) -> Vec<u8> {
    [b";; HSF V19.10 \n".as_slice(), body].concat()
}
fn zlib(body: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(body).unwrap();
    z.finish().unwrap()
}
fn read(data: &[u8], mode: ParseMode) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    parse(
        data,
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
}
fn bad(body: &[u8], code: &str) {
    for mode in [ParseMode::Partial, ParseMode::Strict] {
        let e = read(&file(body), mode).unwrap_err();
        assert_eq!(e.diagnostic.code, code);
        assert_eq!(e.diagnostic.classification, Status::Invalid);
        assert!(e.diagnostic.source.is_some());
    }
}

#[test]
fn independent_golden_operands_preserve_bytes_and_context() {
    // Hand-authored golden bytes, no parser-owned writer.
    let body = b"I\x04\x00\x00\x00;note\n(\x04rootqU\x05\x00x=\xff,y(\x00)v\xff\xff\xff\xff)r\x00\x00\x00\x00s\x00\x00\x00\x00)\0";
    let p = read(&file(body), ParseMode::Strict).unwrap();
    assert_eq!(p.status, Status::Complete);
    assert!(p.unparsed.is_empty());
    assert_eq!(
        p.data
            .records
            .iter()
            .map(|r| r.kind.as_str())
            .collect::<Vec<_>>(),
        [
            "file_info",
            "comment",
            "open_segment",
            "tag_implicit",
            "user_options",
            "open_segment",
            "close_segment",
            "user_value",
            "close_segment",
            "reference",
            "reopen_segment",
            "close_segment",
            "termination"
        ]
    );
    let options = &p.data.records[4];
    assert_eq!(
        p.data.source_bytes(options.payload.as_ref().unwrap()),
        Some(b"x=\xff,y".as_slice())
    );
    assert_eq!(options.scope.as_deref(), Some("record:2"));
    assert_eq!(p.data.records[7].value, Some(u32::MAX));
    assert_eq!(p.data.records[9].value, Some(0));
}

#[test]
fn versioned_include_and_long_options() {
    let p = read(
        &file(b"<\0\x01a\x01b{\x01xU\xff\xff\x03\0\0\0a=b\0"),
        ParseMode::Strict,
    )
    .unwrap();
    assert_eq!(
        p.data
            .source_bytes(p.data.records[0].condition.as_ref().unwrap()),
        Some(b"b".as_slice())
    );
    let old = b";; HSF V8.05 \n<\0\0";
    let p = read(old, ParseMode::Strict).unwrap();
    assert!(p.data.records[0].condition.is_none());
    assert_eq!(p.data.records[0].payload.as_ref().unwrap().length, 0);
    for version in ["8.05", "14.50", "19.10", "019.10"] {
        let data = format!(";; HSF V{version} \n\0");
        assert_eq!(
            read(data.as_bytes(), ParseMode::Strict).unwrap().status,
            Status::Complete
        );
    }
    let data = b";; HSF V20.90 \n\0";
    assert_eq!(
        read(data, ParseMode::Partial).unwrap().status,
        Status::Partial
    );
    assert_eq!(
        read(data, ParseMode::Strict).unwrap_err().diagnostic.code,
        "hsf.unsupported_version"
    );
}

#[test]
fn multiple_compressed_domains_continue_with_outer_syntax() {
    let one = zlib(b"(\x01az");
    let two = zlib(b"U\x03\0x=yz");
    let body = [b"Z".as_slice(), &one, b"qZ", &two, b")\0"].concat();
    let data = file(&body);
    let p = read(&data, ParseMode::Strict).unwrap();
    assert_eq!(p.status, Status::Complete);
    assert_eq!(p.data.domains.len(), 3);
    assert_eq!(p.usage.expanded_bytes, 11);
    assert_eq!(
        p.data
            .source_bytes(p.data.domains[1].parent.as_ref().unwrap()),
        Some(one.as_slice())
    );
    assert_eq!(
        p.data
            .source_bytes(p.data.domains[2].parent.as_ref().unwrap()),
        Some(two.as_slice())
    );
    assert!(p.data.domains.iter().all(|d| d.parsed_bytes == d.byte_len));
    assert_eq!(p.data.records[5].scope.as_deref(), Some("record:1"));
}

#[test]
fn unknown_stops_without_resynchronization_and_keeps_all_remaining_domains() {
    let block = zlib(b";ok\n\xfeI\0\0\0\0z");
    let bytes = file(&[b"Z".as_slice(), &block, b"I\0\0\0\0\0"].concat());
    let p = read(&bytes, ParseMode::Partial).unwrap();
    assert_eq!(p.status, Status::Partial);
    assert_eq!(p.data.records.len(), 2);
    assert_eq!(p.unparsed.len(), 2);
    let diagnostic = &p.diagnostics[0];
    assert_eq!(
        diagnostic.source.as_ref().unwrap(),
        &SourceSpan {
            domain_id: "hsf:1".into(),
            offset: 4,
            length: 1
        }
    );
    assert_eq!(diagnostic.opcode, Some(0xfe));
    assert_eq!(diagnostic.version.as_deref(), Some("19.10"));
    assert_eq!(
        *read(&bytes, ParseMode::Strict).unwrap_err().diagnostic,
        *diagnostic
    );
    let p = read(&file(b"[\xff\xff\xff\xff\0"), ParseMode::Partial).unwrap();
    assert_eq!(p.diagnostics[0].code, "hsf.unsupported_user_data");
    assert!(p.data.records.is_empty());
}

#[test]
fn corruption_is_never_a_successful_partial_parse() {
    for body in [b"I".as_slice(), b"U\xff", b"(\x03a", b"s\x00", b";no LF"] {
        bad(body, "hsf.truncated_operand");
    }
    bad(b"", "hsf.missing_terminator");
    bad(b"z", "hsf.misplaced_terminator");
    bad(b")\0", "hsf.unbalanced_segment");
    bad(b"(\0\0", "hsf.unbalanced_segment");
    bad(b"\0x", "hsf.trailing_bytes");
    for (decoded, code) in [
        (b"I\0\0\0\0".as_slice(), "hsf.missing_terminator"),
        (b"\0", "hsf.misplaced_terminator"),
        (b"zX", "hsf.trailing_bytes"),
    ] {
        bad(&[b"Z".as_slice(), &zlib(decoded), b"\0"].concat(), code);
    }
    let mut compressed = zlib(b"z");
    let last = compressed.len() - 1;
    compressed[last] ^= 0x80;
    bad(
        &[b"Z".as_slice(), &compressed, b"\0"].concat(),
        "hsf.invalid_zlib",
    );
    bad(
        &[b"Z".as_slice(), &zlib(b"z")[..3]].concat(),
        "hsf.truncated_zlib",
    );
}

#[test]
fn budgets_are_shared_and_bound_metadata_before_retention() {
    let decoded = [b";".as_slice(), &[b'a'; 58], b"\nz"].concat();
    let bytes = file(&[b"Z".as_slice(), &zlib(&decoded), b"\0"].concat());
    let mut budget = Budget::new(Limits {
        max_expanded_bytes: 100,
        ..Default::default()
    });
    budget.charge_expanded(60).unwrap();
    let e = parse(&bytes, &ParseOptions::default(), &mut budget).unwrap_err();
    assert_eq!(e.diagnostic.code, "budget.expanded");
    assert_eq!(e.diagnostic.source.as_ref().unwrap().offset, 15);
    assert_eq!(budget.usage().expanded_bytes, 60);
    for options in [
        ParseOptions {
            max_records: 0,
            ..Default::default()
        },
        ParseOptions {
            max_domains: 0,
            ..Default::default()
        },
        ParseOptions {
            max_segment_depth: 0,
            ..Default::default()
        },
        ParseOptions {
            max_string_bytes: 0,
            ..Default::default()
        },
    ] {
        let e = parse(
            &file(b"(\x01a)\0"),
            &options,
            &mut Budget::new(Limits::default()),
        )
        .unwrap_err();
        assert_eq!(e.diagnostic.classification, Status::LimitExceeded);
    }
}

#[test]
fn every_cut_of_a_supported_file_is_invalid_not_a_panic_or_complete() {
    let raw = file(
        &[
            b"I\0\0\0\0Z".as_slice(),
            &zlib(b"(\x01aU\x03\0a=b)z"),
            b"\0",
        ]
        .concat(),
    );
    for i in 0..raw.len() {
        let e = read(&raw[..i], ParseMode::Partial).unwrap_err();
        assert_eq!(e.diagnostic.classification, Status::Invalid, "cut={i}");
    }
    assert!(read(&raw, ParseMode::Strict).is_ok());
}

#[test]
fn compression_ratio_uses_exact_block_not_outer_suffix() {
    let decoded = [b";".as_slice(), &[b'x'; 1000], b"\nz"].concat();
    for suffix in [b"\0".to_vec(), vec![0; 2000]] {
        let raw = file(&[b"Z".as_slice(), &zlib(&decoded), &suffix].concat());
        let options = ParseOptions {
            max_compression_ratio: 1,
            ..Default::default()
        };
        let e = parse(&raw, &options, &mut Budget::new(Limits::default())).unwrap_err();
        assert_eq!(e.diagnostic.code, "hsf.limit_compression_ratio");
        assert_eq!(e.diagnostic.opcode, Some(b'Z'));
    }
}

#[test]
fn bounds_keep_float_bits_and_unknown_representation_stops() {
    let bits = [0xff; 24]; // including non-finite floats: syntax retains raw bits
    let p = read(
        &file(&[b"B\0".as_slice(), &bits, b"b\x01", &[0; 16], b"\0"].concat()),
        ParseMode::Strict,
    )
    .unwrap();
    assert_eq!(
        p.data
            .source_bytes(p.data.records[0].payload.as_ref().unwrap()),
        Some(bits.as_slice())
    );
    assert_eq!(p.data.records[1].value, Some(1));
    let p = read(&file(b"B\xffI\0\0\0\0\0"), ParseMode::Partial).unwrap();
    assert!(p.data.records.is_empty());
    assert_eq!(p.diagnostics[0].code, "hsf.unsupported_bounds");
    assert_eq!(p.unparsed[0].offset, 15);
}
