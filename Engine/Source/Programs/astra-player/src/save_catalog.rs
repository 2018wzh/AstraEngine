use crate::PlayerHostCommandResult;

pub async fn hydrate_save_catalog(
    source: &mut crate::NativeVnHostCommandSource,
    executor: &mut crate::PlayerHostCommandExecutor<crate::PlatformCommandSink>,
) -> Result<(), astra_platform::PlatformError> {
    let results = executor
        .execute_batch(
            source
                .list_saves()
                .map_err(|error| catalog_error("player.save.list.prepare", error))?,
        )
        .await
        .map_err(|error| catalog_error("player.save.list", error))?;
    let slots = match results.as_slice() {
        [PlayerHostCommandResult::SaveList { slots }] => slots.clone(),
        _ => {
            return Err(catalog_error(
                "player.save.list",
                "ASTRA_PLAYER_SAVE_LIST_RESULT_INVALID: platform returned an unexpected result",
            ));
        }
    };
    for slot in &slots {
        let results = executor
            .execute_batch(
                source
                    .read_save(slot)
                    .map_err(|error| catalog_error("player.save.catalog.read.prepare", error))?,
            )
            .await
            .map_err(|error| catalog_error("player.save.catalog.read", error))?;
        let bytes = match results.as_slice() {
            [PlayerHostCommandResult::SaveRead { bytes }] => bytes,
            _ => {
                return Err(catalog_error(
                    "player.save.catalog.read",
                    "ASTRA_PLAYER_SAVE_CATALOG_RESULT_INVALID: platform returned an unexpected result",
                ));
            }
        };
        match source.ingest_save_catalog_entry(slot, bytes) {
            Ok(()) => {}
            Err(crate::NativeVnHostError::Save(_)) => {
                source.reject_save_catalog_entry(slot);
                tracing::warn!(
                    event = "player.save.catalog.rejected",
                    diagnostic_code = "ASTRA_PLAYER_SAVE_CATALOG_REJECTED",
                    "unreadable save remains protected; other slots are available"
                );
            }
            Err(error) => return Err(catalog_error("player.save.catalog.ingest", error)),
        }
    }
    tracing::trace!(
        event = "player.save.catalog.hydrated",
        slot_count = slots.len(),
        "hydrated validated save metadata before launching the product runtime"
    );
    Ok(())
}

fn catalog_error(
    operation: &'static str,
    error: impl std::fmt::Display,
) -> astra_platform::PlatformError {
    astra_platform::PlatformError::new(
        astra_platform::PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}
