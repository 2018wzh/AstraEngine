use super::stage::*;
use super::*;

impl NativeVnHostCommandSource {
    pub(super) fn store_texture(
        &mut self,
        asset_id: String,
        frame: TextureFrame,
        protected: &BTreeSet<String>,
    ) -> Result<(), NativeVnHostError> {
        let incoming_bytes = frame.rgba8.len() as u64;
        if incoming_bytes == 0 || incoming_bytes > self.texture_cpu_budget_bytes {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_CPU_TEXTURE_ENTRY_BUDGET".into(),
            ));
        }
        if let Some(previous) = self.textures.remove(&asset_id) {
            self.texture_cpu_bytes = self
                .texture_cpu_bytes
                .checked_sub(previous.rgba8.len() as u64)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_BYTES_UNDERFLOW".into())
                })?;
        }
        self.texture_use_clock = self
            .texture_use_clock
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        self.texture_cpu_last_used
            .insert(asset_id.clone(), self.texture_use_clock);
        self.texture_cpu_bytes = self
            .texture_cpu_bytes
            .checked_add(incoming_bytes)
            .ok_or_else(|| {
                NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_BYTES_OVERFLOW".into())
            })?;
        self.texture_dimensions
            .insert(asset_id.clone(), (frame.width, frame.height));
        self.textures.insert(asset_id, frame);

        while self.texture_cpu_bytes > self.texture_cpu_budget_bytes {
            let candidate = self
                .textures
                .keys()
                .filter(|candidate| !protected.contains(*candidate))
                .map(|candidate| {
                    (
                        self.texture_cpu_last_used
                            .get(candidate)
                            .copied()
                            .unwrap_or(0),
                        candidate.clone(),
                    )
                })
                .min();
            let Some((_, candidate)) = candidate else {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_CPU_TEXTURE_BUDGET_PINNED".into(),
                ));
            };
            let evicted = self.textures.remove(&candidate).ok_or_else(|| {
                NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_EVICTION_MISSING".into())
            })?;
            self.texture_cpu_bytes = self
                .texture_cpu_bytes
                .checked_sub(evicted.rgba8.len() as u64)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_BYTES_UNDERFLOW".into())
                })?;
            self.texture_cpu_last_used.remove(&candidate);
            tracing::debug!(
                event = "player.image.cpu_residency.evicted",
                resident_bytes = self.texture_cpu_bytes,
                budget_bytes = self.texture_cpu_budget_bytes,
                "evicted an inactive decoded stage texture within the profile-bound CPU budget"
            );
        }
        Ok(())
    }

    pub(super) fn stage_texture_for_viewport(
        &self,
        frame: TextureFrame,
    ) -> Result<TextureFrame, NativeVnHostError> {
        if frame.width <= self.width && frame.height <= self.height {
            return Ok(frame);
        }
        let source = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba8.to_vec())
            .ok_or_else(|| {
                NativeVnHostError::Asset(
                    "ASTRA_PLAYER_STAGE_TEXTURE_DIMENSIONS: decoded frame dimensions are invalid"
                        .into(),
                )
            })?;
        let scale =
            (self.width as f64 / frame.width as f64).min(self.height as f64 / frame.height as f64);
        if !scale.is_finite() || scale <= 0.0 {
            return Err(NativeVnHostError::Asset(
                "ASTRA_PLAYER_STAGE_TEXTURE_VIEWPORT: viewport dimensions are invalid".into(),
            ));
        }
        let target_width = ((frame.width as f64 * scale).floor() as u32).max(1);
        let target_height = ((frame.height as f64 * scale).floor() as u32).max(1);
        let resized = image::imageops::resize(
            &source,
            target_width,
            target_height,
            image::imageops::FilterType::Lanczos3,
        );
        TextureFrame::from_vec(target_width, target_height, resized.into_raw()).map_err(|error| {
            NativeVnHostError::Asset(format!("ASTRA_PLAYER_STAGE_TEXTURE_RESIZE: {error}"))
        })
    }

    pub(super) fn remove_texture(
        &mut self,
        asset_id: &str,
    ) -> Result<Option<TextureFrame>, NativeVnHostError> {
        let removed = self.textures.remove(asset_id);
        if let Some(frame) = &removed {
            self.texture_cpu_bytes = self
                .texture_cpu_bytes
                .checked_sub(frame.rgba8.len() as u64)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_CPU_TEXTURE_BYTES_UNDERFLOW".into())
                })?;
            self.texture_cpu_last_used.remove(asset_id);
        }
        Ok(removed)
    }

    pub(super) fn mark_texture_used(&mut self, asset_id: &str) -> Result<(), NativeVnHostError> {
        self.texture_use_clock = self
            .texture_use_clock
            .checked_add(1)
            .ok_or(NativeVnHostError::SequenceOverflow)?;
        self.texture_last_used
            .insert(asset_id.to_string(), self.texture_use_clock);
        self.texture_cpu_last_used
            .insert(asset_id.to_string(), self.texture_use_clock);
        Ok(())
    }

    pub(super) fn evict_gpu_textures_for(
        &mut self,
        incoming_bytes: u64,
        protected: &BTreeSet<String>,
        lifecycle: &mut Vec<SceneCommand>,
    ) -> Result<(), NativeVnHostError> {
        if incoming_bytes > self.gpu_texture_budget_bytes {
            return Err(NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_GPU_TEXTURE_OVER_BUDGET: {incoming_bytes} > {}",
                self.gpu_texture_budget_bytes
            )));
        }
        let mut evicted_count = 0u64;
        let mut evicted_bytes = 0u64;
        while self
            .resident_texture_bytes
            .checked_add(incoming_bytes)
            .is_none_or(|total| total > self.gpu_texture_budget_bytes)
        {
            let candidate = self
                .live_texture_ids
                .iter()
                .filter(|asset_id| !protected.contains(*asset_id))
                .map(|asset_id| {
                    (
                        self.texture_last_used.get(asset_id).copied().unwrap_or(0),
                        asset_id.clone(),
                    )
                })
                .min();
            let Some((_, asset_id)) = candidate else {
                return Err(NativeVnHostError::Asset(
                    "ASTRA_PLAYER_GPU_TEXTURE_BUDGET_PINNED".into(),
                ));
            };
            let bytes = self
                .live_texture_bytes
                .get(&asset_id)
                .copied()
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_GPU_RESIDENT_METADATA_MISSING".into())
                })?;
            self.resident_texture_bytes = self
                .resident_texture_bytes
                .checked_sub(bytes)
                .ok_or_else(|| {
                    NativeVnHostError::Asset("ASTRA_PLAYER_GPU_RESIDENT_BYTES_UNDERFLOW".into())
                })?;
            self.live_texture_ids.remove(&asset_id);
            self.live_texture_bytes.remove(&asset_id);
            self.texture_last_used.remove(&asset_id);
            lifecycle.push(SceneCommand::ReleaseResource {
                resource_id: asset_id,
            });
            evicted_count += 1;
            evicted_bytes = evicted_bytes.saturating_add(bytes);
        }
        if evicted_count > 0 {
            tracing::debug!(
                event = "player.image.gpu_residency.evicted",
                evicted_count,
                evicted_bytes,
                resident_bytes = self.resident_texture_bytes,
                budget_bytes = self.gpu_texture_budget_bytes,
                "evicted least-recently-used stage textures within the profile-bound budget"
            );
        }
        Ok(())
    }

    pub fn decoded_asset_cache_bytes(&self) -> u64 {
        self.asset_store.cache_bytes()
    }

    pub(super) fn poll_image_prefetch(&mut self) -> Result<(), NativeVnHostError> {
        if let Some(error) = &self.image_prefetch_failure {
            return Err(NativeVnHostError::Asset(error.clone()));
        }
        for completion in self.image_prefetcher.drain_completions()? {
            if !self.image_prefetcher.is_current(&completion.scope) {
                continue;
            }
            let asset_id = completion.asset_id;
            self.image_prefetch_inflight.remove(&asset_id);
            if let Err(error) = completion.result {
                let error = format!("ASTRA_PLAYER_IMAGE_PREFETCH_FAILED: {error}");
                self.image_prefetch_failure = Some(error.clone());
                return Err(NativeVnHostError::Asset(error));
            }
            if self.live_texture_ids.contains(&asset_id) {
                self.asset_store.release_uploaded_image(&asset_id)?;
            }
        }
        Ok(())
    }

    pub(super) fn schedule_current_image_prefetch(&mut self) -> Result<(), NativeVnHostError> {
        self.poll_image_prefetch()?;
        let Some(state_id) = self
            .runtime_state
            .as_ref()
            .and_then(|state| state.cursor.as_ref())
            .map(|cursor| cursor.state_id.as_str())
        else {
            return Ok(());
        };
        let Some(assets) = self.image_prefetch_windows.get(state_id) else {
            self.asset_store.pin_image_working_set(&[])?;
            return Ok(());
        };
        let working_set = current_image_working_set(self.stage_director.state());
        self.asset_store.pin_image_working_set(&working_set)?;
        for asset_id in assets {
            if self.live_texture_ids.contains(asset_id)
                || self.textures.contains_key(asset_id)
                || self.image_prefetch_inflight.contains(asset_id)
                || self.asset_store.is_image_cached(asset_id)?
            {
                continue;
            }
            if !self.image_prefetcher.try_schedule(asset_id.clone())? {
                break;
            }
            self.image_prefetch_inflight.insert(asset_id.clone());
        }
        Ok(())
    }

    pub(super) fn texture(&self, asset_id: &str) -> Result<&TextureFrame, NativeVnHostError> {
        self.textures.get(asset_id).ok_or_else(|| {
            NativeVnHostError::Asset(format!(
                "ASTRA_PLAYER_ASSET_MISSING: cooked texture {asset_id} is not mounted"
            ))
        })
    }
}
