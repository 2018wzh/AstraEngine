use super::*;

pub(super) async fn execute_web_ui_host_request(
    source: &mut NativeVnHostCommandSource,
    executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    save_transaction_id: &mut u64,
) -> Result<(), astra_platform::PlatformError> {
    let Some(request) = source.take_ui_host_request() else {
        return Ok(());
    };
    match request {
        astra_player_vn::VnUiHostRequest::Save { slot_id, .. } => {
            *save_transaction_id = save_transaction_id.checked_add(1).ok_or_else(|| {
                web_player_error(
                    "player.save.transaction",
                    "ASTRA_PLAYER_SAVE_TRANSACTION_OVERFLOW",
                )
            })?;
            execute_web_save(
                source,
                executor,
                &slot_id,
                PlayerHostResourceId(*save_transaction_id),
            )
            .await?;
            if let Some(batch) = source
                .mark_save_committed(&slot_id)
                .map_err(|error| web_player_error("player.save.commit_state", error))?
            {
                executor
                    .execute_batch(batch)
                    .await
                    .map_err(|error| web_player_error("player.save.commit_completion", error))?;
            }
            Ok(())
        }
        astra_player_vn::VnUiHostRequest::Load { slot_id } => {
            execute_web_load(source, executor, &slot_id).await
        }
        astra_player_vn::VnUiHostRequest::Delete { slot_id } => {
            executor
                .execute_batch(
                    source
                        .delete_save(&slot_id)
                        .map_err(|error| web_player_error("player.save.delete.prepare", error))?,
                )
                .await
                .map_err(|error| web_player_error("player.save.delete", error))?;
            source
                .mark_save_deleted(&slot_id)
                .map_err(|error| web_player_error("player.save.delete_state", error))
        }
    }
}

pub(super) async fn execute_web_save(
    source: &mut NativeVnHostCommandSource,
    executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    slot: &str,
    transaction: PlayerHostResourceId,
) -> Result<(), astra_platform::PlatformError> {
    let plan = source
        .prepare_save_transaction(slot, transaction)
        .map_err(|error| web_player_error("player.save.prepare", error))?;
    executor
        .execute_save_transaction(plan)
        .await
        .map_err(|error| web_player_error("player.save.transaction", error))?;
    Ok(())
}

pub(super) async fn execute_web_load(
    source: &mut NativeVnHostCommandSource,
    executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
    slot: &str,
) -> Result<(), astra_platform::PlatformError> {
    let results = executor
        .execute_batch(
            source
                .read_save(slot)
                .map_err(|error| web_player_error("player.save.read.prepare", error))?,
        )
        .await
        .map_err(|error| web_player_error("player.save.read", error))?;
    let bytes = match results.as_slice() {
        [PlayerHostCommandResult::SaveRead { bytes }] => bytes,
        _ => {
            return Err(web_player_error(
                "player.save.read",
                "ASTRA_PLAYER_SAVE_RESULT_INVALID: platform returned an unexpected result",
            ));
        }
    };
    let present = source
        .restore(bytes)
        .map_err(|error| web_player_error("player.save.restore", error))?;
    executor
        .execute_batch(present)
        .await
        .map_err(|error| web_player_error("player.save.present", error))?;
    Ok(())
}

pub(super) fn web_player_error(
    operation: &'static str,
    error: impl std::fmt::Display,
) -> astra_platform::PlatformError {
    astra_platform::PlatformError::new(
        PlatformErrorCode::InvalidState,
        operation,
        error.to_string(),
    )
}

pub(super) fn log_web_consumed_step(
    player_sequence: u64,
    kind: &str,
    identity: &WebEvidenceIdentity,
    source: &NativeVnHostCommandSource,
    media: &NativeVnProductMediaHost,
) -> Result<(), astra_platform::PlatformError> {
    let evidence = source.last_step_evidence().ok_or_else(|| {
        web_player_error(
            "player.runtime.evidence",
            "ASTRA_PLAYER_RUNTIME_EVIDENCE_MISSING",
        )
    })?;
    tracing::info!(
        event = "astra.player.web.runtime.input_consumed",
        player_sequence,
        input_kind = kind,
        fixed_step = evidence.fixed_step,
        session_id = source.session_id(),
        presentation_count = evidence.presentation_count,
        terminal_route_count = evidence.terminal_route_ids.len(),
        pending_choice_count = evidence.pending_choice_ids.len(),
        "Web Player consumed a platform input through RuntimeWorld"
    );
    emit_web_evidence(&WebPlayerLiveEvidence {
        schema: WEB_PLAYER_LIVE_EVIDENCE_SCHEMA.to_string(),
        event: "runtime.input_consumed".to_string(),
        target: identity.target.clone(),
        profile: identity.profile.clone(),
        package_hash: identity.package_hash.clone(),
        package_byte_size: None,
        session_id: Some(source.session_id().to_string()),
        player_sequence: Some(player_sequence),
        input_kind: Some(kind.to_string()),
        fixed_step: Some(evidence.fixed_step),
        coverage_reached: evidence.coverage_reached.iter().cloned().collect(),
        current_state_id: evidence.current_state_id.clone(),
        terminal_route_ids: evidence.terminal_route_ids.iter().cloned().collect(),
        pending_choice_ids: evidence.pending_choice_ids.clone(),
        audio_meter: media.last_audio_meter(),
    })
    .map_err(|error| web_player_error("player.runtime.evidence", error))?;
    Ok(())
}

pub(super) fn emit_web_evidence(value: &WebPlayerLiveEvidence) -> Result<(), String> {
    let encoded = serde_json::to_string(&value)
        .map_err(|error| format!("ASTRA_PLAYER_WEB_EVIDENCE_ENCODE: {error}"))?;
    web_sys::console::info_1(&JsValue::from_str(&format!(
        "ASTRA_PLAYER_EVIDENCE {encoded}"
    )));
    Ok(())
}

pub(super) async fn next_animation_frame() -> Result<(), JsValue> {
    let promise = Promise::new(&mut |resolve, reject| {
        let Some(window) = web_sys::window() else {
            let _ = reject.call1(
                &JsValue::UNDEFINED,
                &JsValue::from_str("window unavailable for animation frame"),
            );
            return;
        };
        let resolve = resolve.clone();
        let callback = Closure::once(move |_timestamp: f64| {
            let _ = resolve.call1(&JsValue::UNDEFINED, &JsValue::UNDEFINED);
        });
        if window
            .request_animation_frame(callback.as_ref().unchecked_ref())
            .is_ok()
        {
            callback.forget();
        } else {
            let _ = reject.call1(
                &JsValue::UNDEFINED,
                &JsValue::from_str("requestAnimationFrame registration failed"),
            );
        }
    });
    JsFuture::from(promise).await.map(|_| ())
}

pub(super) async fn fetch_bytes(path: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let response = JsFuture::from(window.fetch_with_str(path)).await?;
    let response: Response = response.dyn_into()?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!(
            "fetch failed with status {}",
            response.status()
        )));
    }
    let buffer = JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
