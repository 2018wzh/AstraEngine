use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
};

use astra_core::Hash256;
use cosmic_text::{fontdb, FontSystem, LayoutGlyph, SwashCache};

use crate::MediaError;

use super::{
    contract::*,
    layout_engine::layout_uncached,
    validation::{
        load_database, request_cache_key, validate_config, validate_context, validate_family_chain,
    },
};

pub struct CosmicTextLayoutProvider {
    context: FontBindingContext,
    config: TextLayoutConfig,
    state: Mutex<FontState>,
}

pub(super) struct FontState {
    pub(super) fonts: Vec<PackagedFont>,
    pub(super) database: fontdb::Database,
    pub(super) faces: BTreeMap<String, LoadedFace>,
    pub(super) font_systems: BTreeMap<String, FontSystem>,
    pub(super) swash_cache: SwashCache,
    layout_cache: BTreeMap<Hash256, CacheEntry>,
    access_sequence: u64,
    generation: u64,
    hits: u64,
    misses: u64,
}

#[derive(Clone)]
pub(super) struct LoadedFace {
    pub(super) asset_id: String,
    pub(super) family: String,
    pub(super) face_index: u32,
    pub(super) hash: Hash256,
    pub(super) coverage: Vec<UnicodeRange>,
}

struct CacheEntry {
    result: TextLayoutResult,
    last_access: u64,
}

pub(super) struct LoadedDatabase {
    pub(super) database: fontdb::Database,
    pub(super) faces: BTreeMap<String, LoadedFace>,
}

pub(super) struct RawLine {
    pub(super) source: SourceRange,
    pub(super) rtl: bool,
    pub(super) top: f32,
    pub(super) baseline: f32,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) glyphs: Vec<LayoutGlyph>,
    pub(super) source_offset: usize,
}

pub(super) struct RawLayout {
    pub(super) lines: Vec<RawLine>,
    pub(super) overflowed: bool,
    pub(super) locale: String,
}

pub(super) struct LayoutPass {
    pub(super) lines: Vec<RawLine>,
    pub(super) total_lines: usize,
    pub(super) max_width: f32,
}

impl CosmicTextLayoutProvider {
    pub fn new(
        context: FontBindingContext,
        mut fonts: Vec<PackagedFont>,
        config: TextLayoutConfig,
    ) -> Result<Self, MediaError> {
        validate_context(&context)?;
        validate_config(&config)?;
        fonts.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
        let loaded = load_database(&context, &config, &fonts)?;
        tracing::info!(
            target: "astra_media::text",
            event = "text.font_database.created",
            target_id = %context.target,
            profile = %context.profile,
            font_count = fonts.len(),
            face_count = loaded.faces.len(),
        );
        Ok(Self {
            context,
            config,
            state: Mutex::new(FontState {
                fonts,
                database: loaded.database,
                faces: loaded.faces,
                font_systems: BTreeMap::new(),
                swash_cache: SwashCache::new(),
                layout_cache: BTreeMap::new(),
                access_sequence: 0,
                generation: 1,
                hits: 0,
                misses: 0,
            }),
        })
    }

    pub fn install_font(&self, font: PackagedFont) -> Result<(), MediaError> {
        let mut state = self.lock_state()?;
        if state
            .fonts
            .iter()
            .any(|current| current.asset_id == font.asset_id)
        {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_DUPLICATE: font asset id is already installed",
            ));
        }
        let mut fonts = state.fonts.clone();
        fonts.push(font);
        self.replace_fonts_locked(&mut state, fonts)?;
        tracing::info!(
            target: "astra_media::text",
            event = "text.font.installed",
            font_count = state.fonts.len(),
            generation = state.generation,
        );
        Ok(())
    }

    pub fn uninstall_font(&self, asset_id: &str, expected_hash: Hash256) -> Result<(), MediaError> {
        let mut state = self.lock_state()?;
        let index = state
            .fonts
            .iter()
            .position(|font| font.asset_id == asset_id)
            .ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_FONT_UNKNOWN: font asset is not installed")
            })?;
        if state.fonts[index].hash != expected_hash {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_HASH: uninstall hash does not match installed font",
            ));
        }
        let mut fonts = state.fonts.clone();
        fonts.remove(index);
        if fonts.is_empty() {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_EMPTY: font database cannot remove its last packaged font",
            ));
        }
        self.replace_fonts_locked(&mut state, fonts)?;
        tracing::info!(
            target: "astra_media::text",
            event = "text.font.uninstalled",
            font_count = state.fonts.len(),
            generation = state.generation,
        );
        Ok(())
    }

    pub fn replace_font(
        &self,
        asset_id: &str,
        expected_hash: Hash256,
        replacement: PackagedFont,
    ) -> Result<(), MediaError> {
        if replacement.asset_id != asset_id {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_REPLACE_ID: replacement must preserve the asset id",
            ));
        }
        let mut state = self.lock_state()?;
        let index = state
            .fonts
            .iter()
            .position(|font| font.asset_id == asset_id)
            .ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_FONT_UNKNOWN: font asset is not installed")
            })?;
        if state.fonts[index].hash != expected_hash {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_HASH: replacement hash does not match installed font",
            ));
        }
        let mut fonts = state.fonts.clone();
        fonts[index] = replacement;
        self.replace_fonts_locked(&mut state, fonts)?;
        tracing::info!(
            target: "astra_media::text",
            event = "text.font.replaced",
            font_count = state.fonts.len(),
            generation = state.generation,
        );
        Ok(())
    }

    pub fn cache_stats(&self) -> Result<TextLayoutCacheStats, MediaError> {
        let state = self.lock_state()?;
        Ok(TextLayoutCacheStats {
            font_generation: state.generation,
            font_count: state.fonts.len(),
            face_count: state.faces.len(),
            entries: state.layout_cache.len(),
            hits: state.hits,
            misses: state.misses,
        })
    }

    fn replace_fonts_locked(
        &self,
        state: &mut FontState,
        mut fonts: Vec<PackagedFont>,
    ) -> Result<(), MediaError> {
        let loaded = load_database(&self.context, &self.config, &fonts)?;
        fonts.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
        state.fonts = fonts;
        state.database = loaded.database;
        state.faces = loaded.faces;
        state.font_systems.clear();
        state.swash_cache = SwashCache::new();
        state.layout_cache.clear();
        state.generation = state.generation.checked_add(1).ok_or_else(|| {
            MediaError::message("ASTRA_TEXT_FONT_GENERATION: font generation overflow")
        })?;
        Ok(())
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, FontState>, MediaError> {
        self.state.lock().map_err(|_| {
            MediaError::message("ASTRA_TEXT_STATE_POISONED: font database lock was poisoned")
        })
    }
}

impl TextLayoutProvider for CosmicTextLayoutProvider {
    fn layout(&self, request: &TextLayoutRequest) -> Result<TextLayoutResult, MediaError> {
        let span = tracing::debug_span!(
            target: "astra_media::text",
            "text_layout",
            event = "text.layout",
            target_id = %self.context.target,
            profile = %self.context.profile,
            run_count = request.runs.len(),
        );
        let _entered = span.enter();
        super::validation::validate_request(request, &self.config)?;
        let mut state = self.lock_state()?;
        validate_family_chain(request, &state)?;
        let cache_key = request_cache_key(request, &state.fonts)?;
        state.access_sequence = state.access_sequence.checked_add(1).ok_or_else(|| {
            MediaError::message("ASTRA_TEXT_CACHE_SEQUENCE: cache sequence overflow")
        })?;
        let access_sequence = state.access_sequence;
        if let Some(entry) = state.layout_cache.get_mut(&cache_key) {
            entry.last_access = access_sequence;
            let result = entry.result.clone();
            state.hits += 1;
            tracing::trace!(
                target: "astra_media::text",
                event = "text.layout.cache_hit",
                layout_hash = %result.hash,
                cache_entries = state.layout_cache.len(),
            );
            return Ok(result);
        }
        state.misses += 1;
        let result = layout_uncached(request, &mut state, &self.context, &self.config)?;
        if state.layout_cache.len() == self.config.max_cache_entries {
            let oldest = state
                .layout_cache
                .iter()
                .min_by_key(|(key, value)| (value.last_access, **key))
                .map(|(key, _)| *key)
                .ok_or_else(|| {
                    MediaError::message("ASTRA_TEXT_CACHE_STATE: cache eviction had no candidate")
                })?;
            state.layout_cache.remove(&oldest);
        }
        state.layout_cache.insert(
            cache_key,
            CacheEntry {
                result: result.clone(),
                last_access: access_sequence,
            },
        );
        tracing::debug!(
            target: "astra_media::text",
            event = "text.layout.completed",
            layout_hash = %result.hash,
            line_count = result.lines.len(),
            glyph_count = result.shaped_runs.iter().map(|run| run.glyphs.len()).sum::<usize>(),
            resource_count = result.glyph_resources.len(),
            clipped = result.clipped,
            ellipsized = result.ellipsized,
        );
        Ok(result)
    }

    fn layout_hash(&self, request: &TextLayoutRequest) -> Result<Hash256, MediaError> {
        Ok(self.layout(request)?.hash)
    }
}
