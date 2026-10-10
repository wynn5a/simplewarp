use super::*;

/*
0 1 2 3
w a r p
-------
0     4  << the span for the string "warp" is (0, 4)

Spanned {
    item: String::new("warp"),  << warp string
    span: Span::new(0, 4)       << span
}

or >> String::new("warp").spanned(Span::new(0, 4))        */
fn warp() -> Spanned<String> {
    String::from("warp").spanned(Span::new(0, 4))
}

fn empty() -> Spanned<String> {
    String::new().spanned_unknown()
}

#[test]
fn knows_distances() {
    assert!(warp().span.distance() == 4);
    assert!(empty().span.distance() == 0);
}

#[test]
fn clamped_to_pulls_span_inside_source_and_onto_char_boundaries() {
    // `é` is two bytes, so offset 2 is mid-character in "aéb".
    assert_eq!(Span::new(2, 99).clamped_to("aéb"), Span::new(1, 4));
    assert_eq!(Span::new(0, 99).clamped_to("abc"), Span::new(0, 3));
    assert_eq!(Span::new(99, 99).clamped_to("abc"), Span::new(3, 3));
}
