use super::*;

impl CmvsArchive {
    /// Resolve a script call using the core's mounted archive order.
    /// Bare names use the original ASCII case-insensitive first-reader rule.
    pub fn resolve_script_uri(&self, name: &str) -> Result<String, CoreError> {
        resolve_script_name(
            &self.prefix,
            name,
            self.entries
                .values()
                .map(|entry| (entry.archive, entry.uri.as_str())),
        )
    }
}

fn resolve_script_name<'a>(
    prefix: &str,
    name: &str,
    entries: impl Iterator<Item = (usize, &'a str)>,
) -> Result<String, CoreError> {
    let invalid_name = || invalid("ASTRA_EMU_CMVS_SCRIPT_URI", "script call name is invalid");
    if name.is_empty() || name.len() > 512 || name.contains(['\0', ':']) {
        return Err(invalid_name());
    }
    if name.contains(['/', '\\']) {
        let normalized = name.replace('\\', "/");
        let path = normalized.strip_prefix('/').unwrap_or(&normalized);
        let uri = format!("{prefix}{path}");
        validate_archive_uri(prefix, &uri).map_err(|_| invalid_name())?;
        return Ok(uri);
    }
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return Err(invalid_name());
    };
    if stem.is_empty() || extension.is_empty() || name == ".." {
        return Err(invalid_name());
    }
    // The entry map is ordered by URI, not by the original reader order.
    // Select by archive ordinal explicitly so role names cannot change priority.
    entries
        .filter(|(_, uri)| {
            uri.rsplit('/')
                .next()
                .is_some_and(|leaf| leaf.eq_ignore_ascii_case(name))
        })
        .min_by_key(|(archive, _)| *archive)
        .map(|(_, uri)| uri.to_owned())
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_CMVS_SCRIPT_NOT_FOUND",
                "script call matched no mounted entry",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_names_follow_archive_order_instead_of_uri_sort_order() {
        let entries = [(1, "cmvs:/a/start.ps3"), (0, "cmvs:/z/START.PS3")];
        assert_eq!(
            resolve_script_name("cmvs:/", "start.ps3", entries.into_iter()).unwrap(),
            "cmvs:/z/START.PS3"
        );
    }

    #[test]
    fn explicit_paths_normalize_game_separators_without_permitting_escape() {
        for name in ["scene/start.ps3", "scene\\start.ps3", "/scene/start.ps3"] {
            assert_eq!(
                resolve_script_name("cmvs:/", name, std::iter::empty()).unwrap(),
                "cmvs:/scene/start.ps3"
            );
        }
        for name in [
            "",
            "../start.ps3",
            "scene/../start.ps3",
            "//host/start.ps3",
            "C:\\start.ps3",
            "scene//start.ps3",
            "start",
            ".ps3",
            "start.",
            "bad\0.ps3",
        ] {
            assert_eq!(
                resolve_script_name("cmvs:/", name, std::iter::empty())
                    .unwrap_err()
                    .code(),
                "ASTRA_EMU_CMVS_SCRIPT_URI"
            );
        }
    }

    #[test]
    fn missing_script_diagnostic_does_not_echo_private_names() {
        let error =
            resolve_script_name("cmvs:/", "private-title.ps3", std::iter::empty()).unwrap_err();
        assert_eq!(error.code(), "ASTRA_EMU_CMVS_SCRIPT_NOT_FOUND");
        assert!(!error.to_string().contains("private-title"));
    }
}
