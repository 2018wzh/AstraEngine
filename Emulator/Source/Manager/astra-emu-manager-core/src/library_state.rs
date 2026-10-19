//! Library roots, removed games, pinned releases, and small Manager state.

use std::{
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use super::{validate_id, Library, LibraryError, MAX_LOCATION_BYTES, MAX_METADATA_BYTES};

const MAX_RELEASE_ID_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRoot {
    pub path: String,
    pub added_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedGame {
    pub game_id: String,
    pub title: String,
    pub location: String,
    pub excluded_at_unix_ms: i64,
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn validate_root(path: &str) -> Result<(), LibraryError> {
    let parsed = Path::new(path);
    if path.is_empty()
        || path.len() > MAX_LOCATION_BYTES
        || path.contains('\0')
        || !parsed.is_absolute()
        || parsed
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(LibraryError::InvalidLocation);
    }
    Ok(())
}

fn validate_release_id(release_id: &str) -> Result<(), LibraryError> {
    let digits = release_id
        .strip_prefix('r')
        .ok_or(LibraryError::InvalidReleaseId)?;
    if digits.is_empty()
        || release_id.len() > MAX_RELEASE_ID_CHARS
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(LibraryError::InvalidReleaseId);
    }
    Ok(())
}

pub(super) fn write_app_state<T: Serialize + ?Sized>(
    connection: &Connection,
    key: &str,
    value: &T,
) -> Result<(), LibraryError> {
    validate_id(key)?;
    let encoded = serde_json::to_string(value).map_err(|_| LibraryError::Serialization)?;
    if encoded.len() > MAX_METADATA_BYTES {
        return Err(LibraryError::Settings);
    }
    connection.execute(
        "INSERT INTO app_state(key, value_json) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
        params![key, encoded],
    )?;
    Ok(())
}

impl Library {
    pub fn library_roots(&self) -> Result<Vec<LibraryRoot>, LibraryError> {
        let mut statement = self
            .connection
            .prepare("SELECT path, added_at_unix_ms FROM library_root ORDER BY path")?;
        let roots = statement
            .query_map([], |row| {
                Ok(LibraryRoot {
                    path: row.get(0)?,
                    added_at_unix_ms: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(roots)
    }

    /// Add a canonical absolute root. Returns `false` when it is already present.
    pub fn add_library_root(&mut self, path: &str) -> Result<bool, LibraryError> {
        validate_root(path)?;
        let inserted = self.connection.execute(
            "INSERT INTO library_root(path, added_at_unix_ms) VALUES(?1, ?2)
             ON CONFLICT(path) DO NOTHING",
            params![path, now_unix_ms()],
        )?;
        Ok(inserted > 0)
    }

    pub fn remove_library_root(&mut self, path: &str) -> Result<bool, LibraryError> {
        let removed = self
            .connection
            .execute("DELETE FROM library_root WHERE path=?1", [path])?;
        Ok(removed > 0)
    }

    /// Hide a game from the library and from later scans until restored.
    pub fn exclude_game(&mut self, game_id: &str) -> Result<(), LibraryError> {
        validate_id(game_id)?;
        if self.game(game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        self.connection.execute(
            "INSERT INTO excluded_game(game_id, excluded_at_unix_ms) VALUES(?1, ?2)
             ON CONFLICT(game_id) DO NOTHING",
            params![game_id, now_unix_ms()],
        )?;
        Ok(())
    }

    pub fn restore_game(&mut self, game_id: &str) -> Result<bool, LibraryError> {
        validate_id(game_id)?;
        let restored = self
            .connection
            .execute("DELETE FROM excluded_game WHERE game_id=?1", [game_id])?;
        Ok(restored > 0)
    }

    pub fn excluded_games(&self) -> Result<Vec<ExcludedGame>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT game.game_id, COALESCE(game.user_title, game.title), game.location,
                    excluded.excluded_at_unix_ms
             FROM excluded_game AS excluded
             JOIN library_game AS game ON game.game_id=excluded.game_id
             ORDER BY COALESCE(game.user_title, game.title) COLLATE NOCASE, game.game_id",
        )?;
        let games = statement
            .query_map([], |row| {
                Ok(ExcludedGame {
                    game_id: row.get(0)?,
                    title: row.get(1)?,
                    location: row.get(2)?,
                    excluded_at_unix_ms: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(games)
    }

    pub fn game_release(&self, game_id: &str) -> Result<Option<String>, LibraryError> {
        validate_id(game_id)?;
        self.connection
            .query_row(
                "SELECT release_id FROM game_release WHERE game_id=?1",
                [game_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(LibraryError::from)
    }

    /// Pin a VNDB release (`r<decimal>`) for a game; `None` clears the pin.
    pub fn set_game_release(
        &mut self,
        game_id: &str,
        release_id: Option<&str>,
    ) -> Result<(), LibraryError> {
        validate_id(game_id)?;
        let Some(release_id) = release_id else {
            self.connection
                .execute("DELETE FROM game_release WHERE game_id=?1", [game_id])?;
            return Ok(());
        };
        validate_release_id(release_id)?;
        if self.game(game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        self.connection.execute(
            "INSERT INTO game_release(game_id, release_id) VALUES(?1, ?2)
             ON CONFLICT(game_id) DO UPDATE SET release_id=excluded.release_id",
            params![game_id, release_id],
        )?;
        Ok(())
    }

    pub fn app_state<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, LibraryError> {
        validate_id(key)?;
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT value_json FROM app_state WHERE key=?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(|_| LibraryError::Settings))
            .transpose()
    }

    pub fn set_app_state<T: Serialize + ?Sized>(
        &mut self,
        key: &str,
        value: &T,
    ) -> Result<(), LibraryError> {
        write_app_state(&self.connection, key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameRecord;

    fn library_with(ids: &[&str]) -> Library {
        let mut library = Library::in_memory().unwrap();
        for id in ids {
            library
                .add_game(&GameRecord {
                    game_id: (*id).into(),
                    title: format!("Title {id}"),
                    user_title: None,
                    location: format!("location-{id}"),
                    family_id: None,
                    content_fingerprint: None,
                    added_at_unix_ms: 1,
                })
                .unwrap();
        }
        library
    }

    #[test]
    fn library_roots_are_unique_absolute_paths() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_str().unwrap().to_owned();
        let mut library = Library::in_memory().unwrap();
        assert!(library.add_library_root(&root).unwrap());
        assert!(!library.add_library_root(&root).unwrap());
        let roots = library.library_roots().unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, root);
        assert!(roots[0].added_at_unix_ms > 0);
        let parent = format!("{root}{}..", std::path::MAIN_SEPARATOR);
        for invalid in ["", "relative/games", parent.as_str()] {
            assert!(matches!(
                library.add_library_root(invalid),
                Err(LibraryError::InvalidLocation)
            ));
        }
        let long = format!("{root}{}", "a".repeat(MAX_LOCATION_BYTES));
        assert!(library.add_library_root(&long).is_err());
        assert!(library.remove_library_root(&root).unwrap());
        assert!(!library.remove_library_root(&root).unwrap());
        assert!(library.library_roots().unwrap().is_empty());
    }

    #[test]
    fn excluded_game_is_hidden_from_snapshot_and_restorable() {
        let mut library = library_with(&["g1", "g2"]);
        library.exclude_game("g1").unwrap();
        library.exclude_game("g1").unwrap();
        let snapshot = library.library_snapshot().unwrap();
        assert_eq!(
            snapshot
                .games
                .iter()
                .map(|game| game.game_id.as_str())
                .collect::<Vec<_>>(),
            ["g2"]
        );
        let excluded = library.excluded_games().unwrap();
        assert_eq!(excluded.len(), 1);
        assert_eq!(excluded[0].game_id, "g1");
        assert_eq!(excluded[0].title, "Title g1");
        assert_eq!(excluded[0].location, "location-g1");
        assert!(matches!(
            library.exclude_game("absent"),
            Err(LibraryError::GameNotFound)
        ));

        assert!(library.restore_game("g1").unwrap());
        assert!(!library.restore_game("g1").unwrap());
        assert!(library.excluded_games().unwrap().is_empty());
        assert_eq!(library.library_snapshot().unwrap().games.len(), 2);

        library.exclude_game("g2").unwrap();
        assert!(library.remove_game("g2").unwrap());
        assert!(library.excluded_games().unwrap().is_empty());
    }

    #[test]
    fn game_release_accepts_only_vndb_release_ids() {
        let mut library = library_with(&["g1"]);
        assert_eq!(library.game_release("g1").unwrap(), None);
        library.set_game_release("g1", Some("r12345")).unwrap();
        assert_eq!(
            library.game_release("g1").unwrap().as_deref(),
            Some("r12345")
        );
        let too_long = format!("r{}", "1".repeat(64));
        for invalid in ["12345", "r", "rabc", "v123", "r12 3", too_long.as_str()] {
            assert!(matches!(
                library.set_game_release("g1", Some(invalid)),
                Err(LibraryError::InvalidReleaseId)
            ));
        }
        assert_eq!(
            library.game_release("g1").unwrap().as_deref(),
            Some("r12345")
        );
        assert!(matches!(
            library.set_game_release("absent", Some("r1")),
            Err(LibraryError::GameNotFound)
        ));
        library.set_game_release("g1", None).unwrap();
        assert_eq!(library.game_release("g1").unwrap(), None);
    }

    #[test]
    fn app_state_round_trips_typed_values() {
        let mut library = Library::in_memory().unwrap();
        assert_eq!(library.app_state::<bool>("network.allowed").unwrap(), None);
        library.set_app_state("network.allowed", &true).unwrap();
        assert_eq!(
            library.app_state::<bool>("network.allowed").unwrap(),
            Some(true)
        );
        library.set_app_state("network.allowed", &false).unwrap();
        assert_eq!(
            library.app_state::<bool>("network.allowed").unwrap(),
            Some(false)
        );
        assert!(matches!(
            library.app_state::<String>("network.allowed"),
            Err(LibraryError::Settings)
        ));
        assert!(matches!(
            library.set_app_state("bad key", &true),
            Err(LibraryError::InvalidId)
        ));
    }
}
