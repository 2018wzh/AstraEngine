use super::package_range::parse_content_range;

#[test]
fn content_range_parser_rejects_ambiguous_or_out_of_bounds_identity() {
    assert_eq!(parse_content_range("bytes 0-0/42").unwrap(), (0, 0, 42));
    for invalid in [
        "items 0-0/42",
        "bytes */42",
        "bytes 1-0/42",
        "bytes 0-42/42",
        "bytes 0-0/*",
        "bytes 0-0/0",
    ] {
        assert!(parse_content_range(invalid).is_err(), "{invalid}");
    }
}
