/// Canonical safe-symbol predicate shared by schema ids, provider ids,
/// metric ids, filter ids and other registry-style identifiers.
///
/// A safe symbol is non-empty, at most 128 bytes, and limited to ASCII
/// alphanumerics plus `.`, `_` and `-`.
pub fn is_safe_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Path-style variant of [`is_safe_symbol`] that additionally allows `/`
/// separators, for symbols that address nested stage/resource paths.
pub fn is_safe_path_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
}

#[cfg(test)]
mod tests {
    use super::{is_safe_path_symbol, is_safe_symbol};

    #[test]
    fn safe_symbol_bounds_and_charset() {
        assert!(is_safe_symbol("astra.vn.step"));
        assert!(is_safe_symbol("metric_id-1"));
        assert!(!is_safe_symbol(""));
        assert!(!is_safe_symbol("a/b"));
        assert!(!is_safe_symbol("sp ace"));
        assert!(!is_safe_symbol(&"a".repeat(129)));
    }

    #[test]
    fn safe_path_symbol_allows_separator() {
        assert!(is_safe_path_symbol("stage/audio/main"));
        assert!(!is_safe_path_symbol(""));
        assert!(!is_safe_path_symbol("a\\b"));
        assert!(!is_safe_path_symbol(&"a".repeat(129)));
    }
}
