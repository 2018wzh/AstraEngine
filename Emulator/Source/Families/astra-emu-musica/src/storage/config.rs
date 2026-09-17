use super::*;
use crate::MusicaConfigState;
const FILE: &str = "configuration.json";
const SCHEMA: &str = "astra.musica.configuration.v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    schema: String,
    game: Hash256,
    settings: MusicaConfigState,
}
impl Storage {
    pub fn configuration(&self, game: Hash256) -> FamilyResult<Option<MusicaConfigState>> {
        let path = self.named_path(FILE, false)?;
        let file = match File::open(path) {
            Ok(file) => file,
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(invalid()),
        };
        let mut bytes = Vec::new();
        file.take(8193)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() > 8192 {
            return Err(invalid());
        }
        let value: Configuration = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if value.schema != SCHEMA || value.game != game {
            return Err(invalid());
        }
        value.settings.validate().map_err(|_| invalid())?;
        Ok(Some(value.settings))
    }
    pub fn write_configuration(
        &self,
        game: Hash256,
        settings: &MusicaConfigState,
    ) -> FamilyResult<()> {
        settings.validate().map_err(|_| invalid())?;
        self.configuration(game)?;
        let path = self.named_path(FILE, true)?;
        let bytes = serde_json::to_vec(&Configuration {
            schema: SCHEMA.into(),
            game,
            settings: settings.clone(),
        })
        .map_err(|_| invalid())?;
        let fail = || {
            error(
                "ASTRA_EMU_MUSICA_CONFIG_WRITE",
                "native configuration could not be atomically stored",
            )
        };
        let mut file =
            tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(|_| fail())?;
        file.write_all(&bytes)
            .and_then(|_| file.as_file().sync_all())
            .map_err(|_| fail())?;
        file.persist(path).map_err(|_| fail())?;
        Ok(())
    }
}
fn invalid() -> astra_emu_family_api::FamilyError {
    error(
        "ASTRA_EMU_MUSICA_CONFIG_READ",
        "native configuration is unavailable, malformed or belongs to another game",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_configuration_rejects_corrupt_foreign_and_oversized_files_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let storage = Storage::new(root.path()).unwrap();
        let game = Hash256::from_sha256(b"game");
        let other = Hash256::from_sha256(b"other");
        let config = MusicaConfigState::default();
        assert!(storage.configuration(game).unwrap().is_none());
        storage.write_configuration(game, &config).unwrap();
        assert_eq!(storage.configuration(game).unwrap(), Some(config.clone()));
        let path = storage.named_path(FILE, false).unwrap();
        let original = fs::read(&path).unwrap();
        assert!(storage.write_configuration(other, &config).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        for bytes in [b"{".to_vec(), vec![b' '; 8193]] {
            fs::write(&path, &bytes).unwrap();
            assert!(storage.configuration(game).is_err());
            assert!(storage.write_configuration(game, &config).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
