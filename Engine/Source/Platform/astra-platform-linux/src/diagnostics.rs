pub fn diagnostics_root(app_id: &str) -> Result<std::path::PathBuf, String> {
    if app_id.is_empty()
        || !app_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err("diagnostics app id must be a safe symbol".to_string());
    }
    #[cfg(target_os = "linux")]
    {
        let project = directories::ProjectDirs::from("com", "AstraEngine", app_id)
            .ok_or_else(|| "Linux diagnostics root is unavailable".to_string())?;
        Ok(project.data_local_dir().join("diagnostics"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("Linux diagnostics root is unavailable on this platform".to_string())
    }
}
