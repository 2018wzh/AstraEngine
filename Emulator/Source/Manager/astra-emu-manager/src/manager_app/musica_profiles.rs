use super::*;
use astra_emu_manager_core::model::ConfigValue;

const PLUGIN: &str = "astra.emu.musica";
const PREFIX: &str = "manager.musica.";

pub(super) struct ProfileImport {
    game_id: String,
    source: ProfileSource,
}

enum ProfileSource {
    Garbro { path: PathBuf, titles: Vec<String> },
    Key(PathBuf),
}

impl AstraEmuManagerController {
    pub(super) fn musica_profile_available(&self) -> bool {
        self.active.is_none()
            && self
                .selected_case_id
                .as_ref()
                .and_then(|id| self.candidates.get(id))
                .is_some_and(|candidate| candidate.descriptor.plugin_id == PLUGIN)
            && self
                .family_options
                .get("manager.emulator_id")
                .is_none_or(|plugin| plugin == PLUGIN)
    }

    pub(super) fn musica_profile_import_pending(&self) -> bool {
        self.musica_profile_available()
            && self
                .musica_import
                .as_ref()
                .is_some_and(|import| Some(&import.game_id) == self.selected_case_id.as_ref())
    }

    pub(super) fn musica_import_fields(&self) -> Vec<GenericConfigFieldViewModel> {
        if !self.musica_profile_import_pending() {
            return Vec::new();
        }
        let import = self.musica_import.as_ref().expect("pending import exists");
        let field = |id: &str, label: &str, description: String, choices: Vec<String>| {
            let key = format!("{PREFIX}{id}");
            GenericConfigFieldViewModel {
                value: self.family_options.get(&key).cloned().unwrap_or_default(),
                key,
                label: label.into(),
                description,
                kind: if choices.is_empty() { "string" } else { "enum" }.into(),
                enum_values: choices,
                required: true,
                min: 0,
                max: 0,
            }
        };
        match &import.source {
            ProfileSource::Garbro { path, titles } => vec![field(
                "title",
                "GARbro 方案",
                format!("来源：{}；明确选择游戏方案后点击导入。", path.display()),
                titles.clone(),
            )],
            ProfileSource::Key(path) => vec![
                field(
                    "paz_version",
                    "PAZ 版本",
                    format!(
                        "来源：{}；key.toml 不含算法参数，必须明确选择。",
                        path.display()
                    ),
                    vec!["0".into(), "1".into(), "2".into()],
                ),
                field(
                    "index_size_xor",
                    "索引长度 XOR",
                    "填写已确认的十进制值或 0x 前缀十六进制值；不猜测参数。".into(),
                    Vec::new(),
                ),
                field(
                    "rc4_skip_crc",
                    "RC4 跳过 CRC",
                    "按已确认的游戏方案选择 true 或 false。".into(),
                    vec!["true".into(), "false".into()],
                ),
            ],
        }
    }

    pub(super) fn musica_import_changed(&mut self, key: &str, value: &str) -> Result<(), String> {
        if !self.musica_profile_import_pending()
            || value.len() > astra_emu_manager_core::model::MAX_CONFIG_TEXT_BYTES
        {
            return Err("ASTRA_EMU_PROFILE_IMPORT_SELECTION_REQUIRED".into());
        }
        let import = self.musica_import.as_ref().expect("pending import exists");
        let valid = match (&import.source, key.strip_prefix(PREFIX)) {
            (ProfileSource::Garbro { titles, .. }, Some("title")) => {
                titles.iter().any(|title| title == value)
            }
            (ProfileSource::Key(_), Some("paz_version")) => matches!(value, "0" | "1" | "2"),
            (ProfileSource::Key(_), Some("index_size_xor")) => true,
            (ProfileSource::Key(_), Some("rc4_skip_crc")) => matches!(value, "true" | "false"),
            _ => false,
        };
        if !valid {
            return Err("ASTRA_EMU_PROFILE_IMPORT_FIELD".into());
        }
        self.family_options.insert(key.into(), value.into());
        Ok(())
    }

    pub(super) fn musica_profile_action(
        &mut self,
        action: &str,
    ) -> Result<ManagerViewModel, String> {
        if !self.musica_profile_available() {
            return Err("ASTRA_EMU_PROFILE_SELECT_MUSICA_FIRST".into());
        }
        if action == "import" {
            self.import_musica_profile()?;
            return self.model();
        }
        let (label, extension) = match action {
            "json" => ("Musica 私有 JSON 配置", "json"),
            "garbro" => ("GARbro Formats.dat", "dat"),
            "key" => ("Musica key.toml", "toml"),
            _ => return Err("ASTRA_EMU_PROFILE_IMPORT_ACTION".into()),
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(label, &[extension])
            .pick_file()
        else {
            return self.model();
        };
        self.select_musica_profile_source(action, path)?;
        self.model()
    }

    fn select_musica_profile_source(&mut self, action: &str, path: PathBuf) -> Result<(), String> {
        if action == "json" {
            astra_emu_musica::profile_import::validate_profile_file(&path)
                .map_err(|error| error.to_string())?;
            return self.apply_musica_profile(&path);
        }
        let game_id = self.selected_game()?.game_id;
        let source = match action {
            "garbro" => {
                let titles = astra_emu_musica::profile_import::list_garbro_titles(&path)
                    .map_err(|error| error.to_string())?;
                if titles.is_empty() {
                    return Err("ASTRA_EMU_PROFILE_GARBRO_NO_SCHEMES".into());
                }
                ProfileSource::Garbro { path, titles }
            }
            "key" => ProfileSource::Key(path),
            _ => return Err("ASTRA_EMU_PROFILE_IMPORT_ACTION".into()),
        };
        self.family_options
            .retain(|key, _| !key.starts_with(PREFIX));
        self.musica_import = Some(ProfileImport { game_id, source });
        Ok(())
    }

    fn import_musica_profile(&mut self) -> Result<(), String> {
        if !self.musica_profile_import_pending() {
            return Err("ASTRA_EMU_PROFILE_IMPORT_SELECTION_REQUIRED".into());
        }
        let import = self.musica_import.as_ref().expect("pending import exists");
        let value = |id: &str| {
            self.family_options
                .get(&format!("{PREFIX}{id}"))
                .map(String::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("ASTRA_EMU_PROFILE_IMPORT_PARAMETER_REQUIRED: {id}"))
        };
        let directory = self.data_dir.join("private-profiles");
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let output = directory.join(format!("musica-{}.json", uuid::Uuid::new_v4()));
        match &import.source {
            ProfileSource::Garbro { path, titles } => {
                let title = value("title")?;
                if !titles.iter().any(|candidate| candidate == title) {
                    return Err("ASTRA_EMU_PROFILE_IMPORT_TITLE".into());
                }
                astra_emu_musica::profile_import::import_garbro_profile(path, title, &output)
            }
            ProfileSource::Key(path) => {
                let version: u8 = value("paz_version")?
                    .parse()
                    .map_err(|_| "ASTRA_EMU_PROFILE_IMPORT_PAZ_VERSION")?;
                if version > 2 {
                    return Err("ASTRA_EMU_PROFILE_IMPORT_PAZ_VERSION".into());
                }
                let xor = parse_xor(value("index_size_xor")?)?;
                let skip = match value("rc4_skip_crc")? {
                    "true" => true,
                    "false" => false,
                    _ => return Err("ASTRA_EMU_PROFILE_IMPORT_RC4_SKIP_CRC".into()),
                };
                astra_emu_musica::profile_import::import_key_profile(
                    path, version, xor, skip, &output,
                )
            }
        }
        .map_err(|error| error.to_string())?;
        if let Err(error) = self.apply_musica_profile(&output) {
            fs::remove_file(&output).map_err(|cleanup| {
                format!("{error}; ASTRA_EMU_PROFILE_IMPORT_CLEANUP: {cleanup}")
            })?;
            return Err(error);
        }
        Ok(())
    }

    fn apply_musica_profile(&mut self, path: &Path) -> Result<(), String> {
        let game_id = self.selected_game()?.game_id;
        let (plugin, schema) = self.config_schema()?;
        if plugin != PLUGIN {
            return Err("ASTRA_EMU_PROFILE_SELECT_MUSICA_FIRST".into());
        }
        let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
        let path = path.to_str().ok_or("ASTRA_EMU_PROFILE_PATH_UTF8")?;
        let mut entries = self
            .library
            .family_configuration(&plugin, &game_id, &schema)
            .map_err(|error| error.to_string())?;
        let profile = entries
            .iter_mut()
            .find(|entry| entry.id == "profile_file")
            .ok_or("ASTRA_EMU_PROFILE_CONFIG_FIELD")?;
        profile.value = ConfigValue::String(path.into());
        self.library
            .save_family_configuration(&plugin, &game_id, &schema, &entries)
            .map_err(|error| error.to_string())?;
        self.family_options.clear();
        self.family_options
            .insert("manager.config_scope".into(), "game".into());
        self.musica_import = None;
        self.diagnostic = "已将私有配置应用到当前游戏；原始游戏文件未修改。".into();
        Ok(())
    }
}

fn parse_xor(value: &str) -> Result<u32, String> {
    let value = value.trim();
    match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => value.parse(),
    }
    .map_err(|_| {
        "ASTRA_EMU_PROFILE_IMPORT_INDEX_SIZE_XOR: 需要 u32 十进制值或 0x 十六进制值".into()
    })
}
