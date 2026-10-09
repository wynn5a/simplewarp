use super::*;

fn set(body: &str) -> ProgramStatusUpdate {
    match parse(body.as_bytes()) {
        Some(ProgramStatusReport::Set(update)) => update,
        other => panic!("expected a Set report for {body:?}, got {other:?}"),
    }
}

fn b64(text: &str) -> String {
    STANDARD.encode(text)
}

fn path(id: &str) -> RecordPath {
    RecordPath::parse(id).unwrap()
}

#[test]
fn parses_every_state() {
    for (name, state) in [
        ("idle", ProgramState::Idle),
        ("working", ProgramState::Working),
        ("done", ProgramState::Done),
        ("blocked", ProgramState::Blocked),
        ("error", ProgramState::Error),
    ] {
        let update = set(&format!("state={name}"));
        assert_eq!(update.state, state);
        assert!(update.id.is_root());
    }
}

#[test]
fn unknown_state_or_missing_state_drops_the_report() {
    assert_eq!(parse(b"state=sleeping"), None);
    assert_eq!(parse(b"app=cargo"), None);
    assert_eq!(parse(b""), None);
}

#[test]
fn unknown_keys_and_malformed_pairs_are_skipped() {
    let update = set("state=working:future=1:junk:app=cargo:averyveryverylongkeyname=1");
    assert_eq!(update.state, ProgramState::Working);
    assert_eq!(update.app.as_deref(), Some("cargo"));
}

#[test]
fn progress_is_an_integer_0_to_100_on_working_or_blocked() {
    assert_eq!(set("state=working:progress=40").progress, Some(40));
    assert_eq!(set("state=working:progress=0").progress, Some(0));
    assert_eq!(set("state=working:progress=100").progress, Some(100));
    assert_eq!(set("state=blocked:progress=5").progress, Some(5));
    assert_eq!(set("state=working").progress, None);
    for bad in ["101", "-1", "4.5", "+5", "", "abc", "999999999999"] {
        assert_eq!(set(&format!("state=working:progress={bad}")).progress, None);
    }
    assert_eq!(set("state=done:progress=40").progress, None);
}

#[test]
fn kind_only_applies_to_blocked() {
    assert_eq!(
        set("state=blocked:kind=permission").kind,
        Some(BlockedKind::Permission)
    );
    assert_eq!(
        set("state=blocked:kind=question").kind,
        Some(BlockedKind::Question)
    );
    assert_eq!(set("state=blocked:kind=auth").kind, Some(BlockedKind::Auth));
    assert_eq!(set("state=blocked:kind=nope").kind, None);
    assert_eq!(set("state=working:kind=permission").kind, None);
}

#[test]
fn app_must_match_the_grammar() {
    assert_eq!(
        set("state=idle:app=my-app_1.2+x").app.as_deref(),
        Some("my-app_1.2+x")
    );
    assert_eq!(set("state=idle:app=bad/app").app, None);
    assert_eq!(set("state=idle:app=").app, None);
    let long = "a".repeat(33);
    assert_eq!(set(&format!("state=idle:app={long}")).app, None);
}

#[test]
fn id_forms_a_hierarchy() {
    assert_eq!(set("state=idle:id=build/test").id, path("build/test"));
    assert!(path("build/test").is_within(&path("build")));
    assert!(path("build").is_within(&path("build")));
    assert!(path("build").is_within(&RecordPath::root()));
    assert!(!path("buildx").is_within(&path("build")));
    assert!(!path("build").is_within(&path("build/test")));
}

#[test]
fn malformed_id_drops_the_report() {
    for bad in ["", "/a", "a/", "a//b", "a b", "ü"] {
        assert_eq!(
            parse(format!("state=idle:id={bad}").as_bytes()),
            None,
            "{bad:?}"
        );
    }
    let long_segment = "a".repeat(33);
    assert_eq!(
        parse(format!("state=idle:id={long_segment}").as_bytes()),
        None
    );
    assert!(parse(b"state=idle:id=a/b/c/d/e/f/g/h").is_some());
    assert_eq!(parse(b"state=idle:id=a/b/c/d/e/f/g/h/i"), None);
    let too_long = ["abcdefghijklmnop"; 9].join("/");
    assert!(too_long.len() > 128);
    assert_eq!(parse(format!("state=idle:id={too_long}").as_bytes()), None);
}

#[test]
fn clear_addresses_a_subtree_or_everything() {
    assert_eq!(
        parse(b"state=clear:id=us-east"),
        Some(ProgramStatusReport::Clear {
            id: path("us-east")
        })
    );
    assert_eq!(
        parse(b"state=clear"),
        Some(ProgramStatusReport::Clear {
            id: RecordPath::root()
        })
    );
}

#[test]
fn text_decodes_with_and_without_padding() {
    let padded = b64("ab");
    assert!(padded.ends_with('='));
    assert_eq!(
        set(&format!("state=idle:msg={padded}")).message.as_deref(),
        Some("ab")
    );
    let unpadded = padded.trim_end_matches('=');
    assert_eq!(
        set(&format!("state=idle:msg={unpadded}"))
            .message
            .as_deref(),
        Some("ab")
    );
    assert_eq!(
        set(&format!("state=idle:title={}", b64("héllo ✓")))
            .title
            .as_deref(),
        Some("héllo ✓")
    );
}

#[test]
fn bad_text_drops_the_whole_report() {
    assert_eq!(parse(b"state=idle:msg=!!!not-base64"), None);
    for text in [
        "a\nb",
        "a\u{1b}[31mb",
        "a\u{7f}",
        "a\u{85}",
        "a\u{202e}b",
        "a\u{2028}b",
    ] {
        let body = format!("state=idle:msg={}", b64(text));
        assert_eq!(parse(body.as_bytes()), None, "{text:?}");
    }
    let invalid_utf8 = STANDARD.encode([0xff, 0xfe]);
    assert_eq!(
        parse(format!("state=idle:msg={invalid_utf8}").as_bytes()),
        None
    );
}

#[test]
fn empty_text_is_absent() {
    let update = set("state=idle:msg=:title=");
    assert_eq!(update.message, None);
    assert_eq!(update.title, None);
}

#[test]
fn text_length_limits() {
    let msg_ok = "a".repeat(2048);
    assert!(
        set(&format!("state=idle:msg={}", b64(&msg_ok)))
            .message
            .is_some()
    );
    let msg_long = "a".repeat(2049);
    assert_eq!(
        parse(format!("state=idle:msg={}", b64(&msg_long)).as_bytes()),
        None
    );

    let title_ok = "a".repeat(192);
    assert!(
        set(&format!("state=idle:title={}", b64(&title_ok)))
            .title
            .is_some()
    );
    let title_long = "a".repeat(193);
    assert_eq!(
        parse(format!("state=idle:title={}", b64(&title_long)).as_bytes()),
        None
    );
}

#[test]
fn sequence_size_limit() {
    let mut body = String::from("state=idle:app=a");
    body.push_str(&":x=y".repeat((MAX_SEQUENCE_BYTES - body.len()) / 4));
    while body.len() < MAX_SEQUENCE_BYTES {
        body.push('z');
    }
    assert_eq!(body.len(), MAX_SEQUENCE_BYTES);
    assert!(parse(body.as_bytes()).is_some());
    body.push('z');
    assert_eq!(parse(body.as_bytes()), None);
}

#[test]
fn non_utf8_body_is_dropped() {
    assert_eq!(parse(b"state=idle:app=\xff"), None);
}

#[test]
fn debug_output_omits_untrusted_text() {
    let update = set(&format!("state=idle:msg={}", b64("secret")));
    assert!(!format!("{update:?}").contains("secret"));
}

fn conemu(params: &[&[u8]]) -> Option<ProgramStatusReport> {
    parse_conemu_progress(params)
}

fn root_update(state: ProgramState, progress: Option<u8>) -> Option<ProgramStatusReport> {
    Some(ProgramStatusReport::Set(ProgramStatusUpdate {
        id: RecordPath::root(),
        state,
        kind: None,
        progress,
        app: None,
        title: None,
        message: None,
    }))
}

#[test]
fn conemu_progress_maps_onto_the_root_record() {
    assert_eq!(
        conemu(&[b"0"]),
        Some(ProgramStatusReport::Clear {
            id: RecordPath::root()
        })
    );
    assert_eq!(
        conemu(&[b"1", b"40"]),
        root_update(ProgramState::Working, Some(40))
    );
    assert_eq!(
        conemu(&[b"1", b"250"]),
        root_update(ProgramState::Working, Some(100))
    );
    assert_eq!(conemu(&[b"1"]), root_update(ProgramState::Working, None));
    assert_eq!(
        conemu(&[b"2", b"40"]),
        root_update(ProgramState::Error, None)
    );
    assert_eq!(conemu(&[b"3"]), root_update(ProgramState::Working, None));
    assert_eq!(
        conemu(&[b"4", b"10"]),
        root_update(ProgramState::Working, None)
    );
}

#[test]
fn conemu_progress_ignores_unknown_or_malformed_states() {
    let cases: [&[&[u8]]; 5] = [&[], &[b"5"], &[b""], &[b"x"], &[b"-1"]];
    for params in cases {
        assert_eq!(conemu(params), None);
    }
}
