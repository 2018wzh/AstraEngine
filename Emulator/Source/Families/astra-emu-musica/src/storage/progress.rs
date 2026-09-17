use super::*;
const FILE: &str = "global-progress.json";
const SCHEMA: &str = "astra.musica.global_progress.v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Progress {
    schema: String,
    game: Hash256,
    unlocks: Vec<Hash256>,
}
fn invalid() -> astra_emu_family_api::FamilyError {
    error(
        "ASTRA_EMU_MUSICA_GLOBAL_PROGRESS",
        "global progress is invalid or belongs to another game",
    )
}
impl Storage {
    pub fn progress(&self, game: Hash256) -> FamilyResult<Vec<Hash256>> {
        let file = match File::open(self.named_path(FILE, false)?) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_PROGRESS_READ",
                    "global progress could not be read",
                ))
            }
        };
        let mut bytes = Vec::new();
        file.take(1025)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() > 1024 {
            return Err(invalid());
        }
        let progress: Progress = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if progress.schema != SCHEMA || progress.game != game {
            return Err(invalid());
        }
        crate::runtime::progress::validate(&progress.unlocks).map_err(|_| invalid())?;
        Ok(progress.unlocks)
    }
    pub fn write_progress(&self, game: Hash256, unlocks: &[Hash256]) -> FamilyResult<()> {
        crate::runtime::progress::validate(unlocks).map_err(|_| invalid())?;
        let existing = self.progress(game)?;
        if existing.iter().any(|id| !unlocks.contains(id)) {
            return Err(invalid());
        }
        let bytes = serde_json::to_vec(&Progress {
            schema: SCHEMA.into(),
            game,
            unlocks: unlocks.to_vec(),
        })
        .map_err(|_| invalid())?;
        let path = self.named_path(FILE, true)?;
        let fail = || {
            error(
                "ASTRA_EMU_MUSICA_PROGRESS_WRITE",
                "global progress could not be atomically stored",
            )
        };
        let mut file =
            tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(|_| fail())?;
        file.write_all(&bytes)
            .and_then(|_| file.as_file().sync_all())
            .map_err(|_| fail())?;
        file.persist(path).map_err(|_| fail())?;
        tracing::info!(
            event = "astra.emu.musica.progress.saved",
            count = unlocks.len()
        );
        Ok(())
    }
}
