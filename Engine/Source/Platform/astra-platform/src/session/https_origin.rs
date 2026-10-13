pub(super) fn https_origin(value: &str) -> Option<String> {
    let rest = value.strip_prefix("https://")?;
    let authority = rest.split('/').next()?;
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains('\\')
        || authority.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(format!("https://{}", authority.to_ascii_lowercase()))
}
