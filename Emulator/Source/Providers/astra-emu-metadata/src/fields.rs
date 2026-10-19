pub(crate) const MAX_ENTRIES: usize = 100_000;

pub(crate) fn is_vndb_vn_id(value: &str) -> bool {
    prefixed_digits(value, 'v')
}

pub(crate) fn is_vndb_release_id(value: &str) -> bool {
    prefixed_digits(value, 'r')
}

fn prefixed_digits(value: &str, prefix: char) -> bool {
    value.len() <= 64
        && value.strip_prefix(prefix).is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub(crate) fn is_plugin_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

pub(crate) fn is_platform(value: &str) -> bool {
    matches!(value, "windows" | "linux" | "macos" | "android")
}

pub(crate) fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn is_bounded_text(value: &str, min_chars: usize, max_chars: usize) -> bool {
    let count = value.chars().count();
    (min_chars..=max_chars).contains(&count) && !value.chars().any(char::is_control)
}

pub(crate) fn is_bounded_multiline(value: &str, max_chars: usize) -> bool {
    value.chars().count() <= max_chars
        && !value
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
}
