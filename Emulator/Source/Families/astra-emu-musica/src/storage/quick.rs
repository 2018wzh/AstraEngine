use super::*;
const CURSOR_FILE: &str = "quick-cursor.json";
const CURSOR_SCHEMA: &str = "astra.musica.quick_cursor.v1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema: String,
    game: Hash256,
    next: u32,
}
impl Storage {
    pub fn quick_cursor(&self, game: Hash256) -> FamilyResult<u32> {
        let path = self.named_path(CURSOR_FILE, false)?;
        let file = match File::open(path) {
            Ok(file) => file,
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(_) => {
                return Err(error(
                    "ASTRA_EMU_MUSICA_QUICK_CURSOR_READ",
                    "quick save cursor could not be read",
                ))
            }
        };
        let mut bytes = Vec::new();
        file.take(1025).read_to_end(&mut bytes).map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_QUICK_CURSOR_READ",
                "quick save cursor could not be read",
            )
        })?;
        let invalid = || {
            error(
                "ASTRA_EMU_MUSICA_QUICK_CURSOR",
                "quick save cursor is malformed or belongs to another game",
            )
        };
        if bytes.len() > 1024 {
            return Err(invalid());
        }
        let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if cursor.schema != CURSOR_SCHEMA || cursor.game != game || cursor.next >= SAVE_PAGE_WIDTH {
            return Err(invalid());
        }
        Ok(cursor.next)
    }
    pub fn write_quick_cursor(&self, game: Hash256, next: u32) -> FamilyResult<()> {
        if next >= SAVE_PAGE_WIDTH {
            return Err(error(
                "ASTRA_EMU_MUSICA_QUICK_CURSOR",
                "quick cursor is outside the rotation range",
            ));
        }
        self.quick_cursor(game)?;
        let path = self.named_path(CURSOR_FILE, true)?;
        let bytes = serde_json::to_vec(&Cursor {
            schema: CURSOR_SCHEMA.into(),
            game,
            next,
        })
        .map_err(|_| {
            error(
                "ASTRA_EMU_MUSICA_QUICK_CURSOR",
                "quick cursor could not be encoded",
            )
        })?;
        let fail = || {
            error(
                "ASTRA_EMU_MUSICA_QUICK_CURSOR_WRITE",
                "quick cursor could not be atomically stored",
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
