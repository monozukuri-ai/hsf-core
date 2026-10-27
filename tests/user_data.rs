use hsf_core::*;
use std::sync::Arc;

struct Profile {
    length: u32,
    field_end: u64,
}
impl UserDataProfile for Profile {
    fn frame(&self, c: UserDataContext<'_>, _: &[u8]) -> Result<Option<UserDataLayout>, HsfError> {
        Ok(Some(UserDataLayout {
            profile: "test-only".into(),
            declared_length: 2,
            computed_payload_length: self.length,
            fields: vec![UserDataField {
                label: "test".into(),
                source: SourceSpan {
                    domain_id: c.domain_id.into(),
                    offset: c.offset,
                    length: self.field_end,
                },
            }],
        }))
    }
}
fn run(
    profile: Option<&dyn UserDataProfile>,
    mode: ParseMode,
    output: u64,
) -> Result<ParseOutcome<HsfDocument>, HsfError> {
    let bytes: Arc<[u8]> = Arc::from(b";; HSF V19.10 \n[\x02\0\0\0xy]U\x03\0x=y\0".as_slice());
    let mut budget = Budget::new(Limits {
        max_output_bytes: output,
        ..Default::default()
    });
    budget.charge_input(bytes.len() as u64)?;
    budget.charge_output(bytes.len() as u64)?;
    parse_precharged_with_user_data(
        bytes,
        &ParseOptions {
            mode,
            ..Default::default()
        },
        &mut budget,
        profile,
    )
}
#[test]
fn hook_is_explicit_and_never_silent_or_strict_success() {
    let profile = Profile {
        length: 2,
        field_end: 8,
    };
    assert!(
        run(None, ParseMode::Partial, 100_000)
            .unwrap()
            .data
            .records
            .is_empty()
    );
    let p = run(Some(&profile), ParseMode::Partial, 100_000).unwrap();
    assert_eq!(p.status, Status::Partial);
    assert_eq!(p.data.records.len(), 3);
    assert!(p.unparsed.is_empty());
    assert_eq!(p.diagnostics[0].code, "hsf.experimental_user_data");
    assert_eq!(
        run(Some(&profile), ParseMode::Strict, 100_000)
            .unwrap_err()
            .diagnostic
            .classification,
        Status::Unsupported
    );
    let s = scene(
        &p,
        &SceneOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert_eq!(s.status, Status::Partial);
    assert!(
        s.diagnostics
            .iter()
            .any(|d| d.code == "hsf.user_data_semantics")
    );
}
#[test]
fn adapter_ranges_and_stop_are_independently_validated() {
    for p in [
        Profile {
            length: 1,
            field_end: 0,
        },
        Profile {
            length: u32::MAX,
            field_end: 0,
        },
        Profile {
            length: 2,
            field_end: u64::MAX,
        },
    ] {
        assert_eq!(
            run(Some(&p), ParseMode::Partial, 100_000)
                .unwrap_err()
                .diagnostic
                .code,
            "hsf.invalid_user_data_profile"
        );
    }
}
#[test]
fn metadata_budget_is_charged_before_callback() {
    struct Never;
    impl UserDataProfile for Never {
        fn frame(
            &self,
            _: UserDataContext<'_>,
            _: &[u8],
        ) -> Result<Option<UserDataLayout>, HsfError> {
            panic!("budget should stop before allocating adapter metadata")
        }
    }
    assert_eq!(
        run(Some(&Never), ParseMode::Partial, 1000)
            .unwrap_err()
            .diagnostic
            .classification,
        Status::LimitExceeded
    );
}

#[test]
fn extended_callback_window_is_borrowed_bounded_and_opt_in() {
    struct Window(usize);
    impl UserDataProfile for Window {
        fn max_framing_bytes(&self) -> usize {
            self.0
        }
        fn frame(
            &self,
            _: UserDataContext<'_>,
            bytes: &[u8],
        ) -> Result<Option<UserDataLayout>, HsfError> {
            assert_eq!(bytes.len(), self.0.min(1 << 20));
            assert_eq!(bytes.first(), Some(&b'['));
            Ok(None)
        }
    }
    for request in [4096, 16384, usize::MAX] {
        let mut input = b";; HSF V19.10 \n[".to_vec();
        input.extend(vec![0; (1 << 20) + 32]);
        let result = parse_precharged_with_user_data(
            Arc::from(input),
            &ParseOptions::default(),
            &mut Budget::new(Limits::default()),
            Some(&Window(request)),
        )
        .unwrap();
        assert_eq!(result.status, Status::Partial);
        assert!(result.data.records.is_empty());
    }
}

#[test]
fn inventory_authorization_is_exact_root_only_and_never_emits_placements() {
    let profile = Profile {
        length: 2,
        field_end: 8,
    };
    let ud = b"[\x02\0\0\0xy]";
    let mut line = vec![b'l'];
    for f in [1f32, 2., 3., 4., 5., 6.] {
        line.extend(f.to_le_bytes());
    }
    for (prefix, suffix, record_id, allowed) in [
        (
            vec![],
            [b"h\0".as_slice(), &line, b"\0"].concat(),
            "record:0",
            true,
        ),
        (vec![], [line.as_slice(), b"\0"].concat(), "record:1", false),
        (line.clone(), vec![0], "record:1", false),
        (b"(\x01a".to_vec(), b")\0".to_vec(), "record:1", false),
    ] {
        let bytes: Arc<[u8]> =
            Arc::from([b";; HSF V19.10 \n".as_slice(), &prefix, ud, &suffix].concat());
        let mut budget = Budget::new(Limits::default());
        budget.charge_input(bytes.len() as u64).unwrap();
        budget.charge_output(bytes.len() as u64).unwrap();
        let p = parse_precharged_with_user_data(
            bytes,
            &ParseOptions::default(),
            &mut budget,
            Some(&profile),
        )
        .unwrap();
        let s = scene(
            &p,
            &SceneOptions {
                inventory_user_data_record: Some(record_id.into()),
                ..Default::default()
            },
            &mut budget,
        )
        .unwrap();
        assert_eq!(s.data.placement_status, Status::Unsupported);
        assert!(s.data.instances.is_empty() && s.data.primitive_instances.is_empty());
        assert_eq!(s.data.opaque_user_data.len(), usize::from(allowed));
        if allowed {
            assert_eq!(s.data.processed_records, p.data.records.len() as u64);
            assert_eq!(s.data.primitives.len(), 1);
            assert!(
                s.diagnostics
                    .iter()
                    .any(|d| d.code == "hsf.user_data_inventory")
            );
        } else {
            assert!(
                s.diagnostics
                    .iter()
                    .any(|d| d.code == "hsf.user_data_semantics")
            );
        }
    }
}

fn terminal_document(middle: &[u8], after: &[u8]) -> ParseOutcome<HsfDocument> {
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;
    let ud = b"[\x02\0\0\0xy]";
    let body = [ud.as_slice(), b"h\0", middle, ud, after, b"z"].concat();
    let mut z = ZlibEncoder::new(vec![], Compression::default());
    z.write_all(&body).unwrap();
    let bytes: Arc<[u8]> =
        Arc::from([b";; HSF V19.10 \nZ".as_slice(), &z.finish().unwrap(), b"\0"].concat());
    parse_precharged_with_user_data(
        bytes,
        &ParseOptions::default(),
        &mut Budget::new(Limits::default()),
        Some(&Profile {
            length: 2,
            field_end: 8,
        }),
    )
    .unwrap()
}

#[test]
fn terminal_inventory_needs_both_authorizations_and_exact_final_record() {
    let p = terminal_document(b"\x01", b"");
    let ids: Vec<_> = p
        .data
        .records
        .iter()
        .filter(|r| r.user_data.is_some())
        .map(|r| r.id.clone())
        .collect();
    for terminal in [None, Some(ids[0].clone()), Some(ids[1].clone())] {
        let accepted = terminal.as_ref() == Some(&ids[1]);
        let s = scene(
            &p,
            &SceneOptions {
                inventory_user_data_record: Some(ids[0].clone()),
                inventory_terminal_user_data_record: terminal,
                ..Default::default()
            },
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        assert_eq!(s.data.opaque_user_data.len(), if accepted { 2 } else { 1 });
        assert_eq!(
            s.data.processed_records == p.data.records.len() as u64,
            accepted
        );
        assert_eq!(s.data.placement_status, Status::Unsupported);
        assert!(s.data.instances.is_empty() && s.data.primitive_instances.is_empty());
    }
    let s = scene(
        &p,
        &SceneOptions {
            inventory_terminal_user_data_record: Some(ids[1].clone()),
            ..Default::default()
        },
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    assert!(s.data.opaque_user_data.is_empty());
}

#[test]
fn terminal_authorization_cannot_hide_following_opcodes_or_open_scopes() {
    for (before, after) in [(b"\x01".as_slice(), b"h\0".as_slice()), (b"(\x01a", b")")] {
        let p = terminal_document(before, after);
        let ids: Vec<_> = p
            .data
            .records
            .iter()
            .filter(|r| r.user_data.is_some())
            .map(|r| r.id.clone())
            .collect();
        let s = scene(
            &p,
            &SceneOptions {
                inventory_user_data_record: Some(ids[0].clone()),
                inventory_terminal_user_data_record: Some(ids[1].clone()),
                ..Default::default()
            },
            &mut Budget::new(Limits::default()),
        )
        .unwrap();
        assert_eq!(s.data.opaque_user_data.len(), 1);
        assert_eq!(
            s.diagnostics.last().unwrap().code,
            "hsf.user_data_semantics"
        );
    }
}

#[test]
fn callback_context_exposes_full_domain_length_scope_and_parent_suffix() {
    use flate2::{Compression, write::ZlibEncoder};
    use std::{io::Write, sync::Mutex};
    type Observation = (u64, usize, Vec<u8>, usize);
    struct Observe(Mutex<Vec<Observation>>);
    impl UserDataProfile for Observe {
        fn frame(
            &self,
            c: UserDataContext<'_>,
            bytes: &[u8],
        ) -> Result<Option<UserDataLayout>, HsfError> {
            self.0.lock().unwrap().push((
                c.domain_byte_len,
                c.segment_depth,
                c.parent_suffix.unwrap().to_vec(),
                bytes.len(),
            ));
            Ok(None)
        }
    }
    let mut body = b"(\x01a[".to_vec();
    body.extend(vec![0; 5000]);
    body.extend(b")z");
    let mut z = ZlibEncoder::new(vec![], Compression::default());
    z.write_all(&body).unwrap();
    let bytes: Arc<[u8]> = Arc::from(
        [
            b";; HSF V19.10 \nZ".as_slice(),
            &z.finish().unwrap(),
            b"h\0\0",
        ]
        .concat(),
    );
    let observe = Observe(Mutex::new(vec![]));
    parse_precharged_with_user_data(
        bytes,
        &ParseOptions::default(),
        &mut Budget::new(Limits::default()),
        Some(&observe),
    )
    .unwrap();
    assert_eq!(
        *observe.0.lock().unwrap(),
        vec![(body.len() as u64, 1, b"h\0\0".to_vec(), 4096)]
    );
}
