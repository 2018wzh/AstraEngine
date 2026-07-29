use std::{
    collections::BTreeMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex, MutexGuard,
    },
};

use astra_core::Hash256;
use astra_worker_budget::WorkerBudgetBroker;
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
    state: Mutex<ProviderState>,
    worker_cursor: AtomicUsize,
    active_workers: AtomicUsize,
    peak_active_workers: AtomicUsize,
}

struct ProviderState {
    catalog: FontState,
    workers: Vec<Arc<Mutex<FontState>>>,
    in_flight: BTreeMap<Hash256, Arc<LayoutFlight>>,
}

struct LayoutFlight {
    result: Mutex<Option<Result<Arc<TextLayoutResult>, String>>>,
    ready: Condvar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLayoutConcurrencyStats {
    pub worker_count: usize,
    pub active_workers: usize,
    pub peak_active_workers: usize,
    pub in_flight_requests: usize,
}

struct ActiveWorkerGuard<'a> {
    active: &'a AtomicUsize,
}

impl Drop for ActiveWorkerGuard<'_> {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
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
    result: Arc<TextLayoutResult>,
    last_access: u64,
}

impl LayoutFlight {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn wait(&self) -> Result<Arc<TextLayoutResult>, MediaError> {
        let mut result = self.result.lock().map_err(|_| {
            MediaError::message("ASTRA_TEXT_SINGLE_FLIGHT_POISONED: result lock was poisoned")
        })?;
        while result.is_none() {
            result = self.ready.wait(result).map_err(|_| {
                MediaError::message("ASTRA_TEXT_SINGLE_FLIGHT_POISONED: wait lock was poisoned")
            })?;
        }
        match result
            .as_ref()
            .expect("single-flight wait observed completion")
        {
            Ok(layout) => Ok(Arc::clone(layout)),
            Err(message) => Err(MediaError::message(message.clone())),
        }
    }

    fn complete(&self, result: Result<Arc<TextLayoutResult>, String>) -> Result<(), MediaError> {
        let mut slot = self.result.lock().map_err(|_| {
            MediaError::message("ASTRA_TEXT_SINGLE_FLIGHT_POISONED: result lock was poisoned")
        })?;
        if slot.is_some() {
            return Err(MediaError::message(
                "ASTRA_TEXT_SINGLE_FLIGHT_DUPLICATE: layout flight completed twice",
            ));
        }
        *slot = Some(result);
        self.ready.notify_all();
        Ok(())
    }
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
        let catalog = FontState {
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
        };
        let worker_count = text_worker_count(&context);
        let workers = (0..worker_count)
            .map(|_| Arc::new(Mutex::new(font_worker_from_catalog(&catalog))))
            .collect();
        Ok(Self {
            context,
            config,
            state: Mutex::new(ProviderState {
                catalog,
                workers,
                in_flight: BTreeMap::new(),
            }),
            worker_cursor: AtomicUsize::new(0),
            active_workers: AtomicUsize::new(0),
            peak_active_workers: AtomicUsize::new(0),
        })
    }

    pub fn concurrency_stats(&self) -> Result<TextLayoutConcurrencyStats, MediaError> {
        let state = self.lock_state()?;
        Ok(TextLayoutConcurrencyStats {
            worker_count: state.workers.len(),
            active_workers: self.active_workers.load(Ordering::Acquire),
            peak_active_workers: self.peak_active_workers.load(Ordering::Acquire),
            in_flight_requests: state.in_flight.len(),
        })
    }

    pub fn install_font(&self, font: PackagedFont) -> Result<(), MediaError> {
        let mut state = self.lock_state()?;
        if state
            .catalog
            .fonts
            .iter()
            .any(|current| current.asset_id == font.asset_id)
        {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_DUPLICATE: font asset id is already installed",
            ));
        }
        let mut fonts = state.catalog.fonts.clone();
        fonts.push(font);
        self.replace_fonts_locked(&mut state, fonts)?;
        tracing::info!(
            target: "astra_media::text",
            event = "text.font.installed",
            font_count = state.catalog.fonts.len(),
            generation = state.catalog.generation,
        );
        Ok(())
    }

    pub fn uninstall_font(&self, asset_id: &str, expected_hash: Hash256) -> Result<(), MediaError> {
        let mut state = self.lock_state()?;
        let index = state
            .catalog
            .fonts
            .iter()
            .position(|font| font.asset_id == asset_id)
            .ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_FONT_UNKNOWN: font asset is not installed")
            })?;
        if state.catalog.fonts[index].hash != expected_hash {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_HASH: uninstall hash does not match installed font",
            ));
        }
        let mut fonts = state.catalog.fonts.clone();
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
            font_count = state.catalog.fonts.len(),
            generation = state.catalog.generation,
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
            .catalog
            .fonts
            .iter()
            .position(|font| font.asset_id == asset_id)
            .ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_FONT_UNKNOWN: font asset is not installed")
            })?;
        if state.catalog.fonts[index].hash != expected_hash {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_HASH: replacement hash does not match installed font",
            ));
        }
        let mut fonts = state.catalog.fonts.clone();
        fonts[index] = replacement;
        self.replace_fonts_locked(&mut state, fonts)?;
        tracing::info!(
            target: "astra_media::text",
            event = "text.font.replaced",
            font_count = state.catalog.fonts.len(),
            generation = state.catalog.generation,
        );
        Ok(())
    }

    pub fn cache_stats(&self) -> Result<TextLayoutCacheStats, MediaError> {
        let state = self.lock_state()?;
        Ok(TextLayoutCacheStats {
            font_generation: state.catalog.generation,
            font_count: state.catalog.fonts.len(),
            face_count: state.catalog.faces.len(),
            entries: state.catalog.layout_cache.len(),
            hits: state.catalog.hits,
            misses: state.catalog.misses,
        })
    }

    /// Drops shaped-layout entries without rebuilding the packaged font database.
    ///
    /// Exhaustive validation requests use identities that the interactive UI
    /// never reuses. They must not occupy the bounded runtime LRU and evict
    /// product layouts before the first frame is rendered.
    pub fn clear_layout_cache(&self) -> Result<(), MediaError> {
        let mut state = self.lock_state()?;
        if !state.in_flight.is_empty() {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_GENERATION_BUSY: cannot clear layout cache during shaping",
            ));
        }
        state.catalog.layout_cache.clear();
        state.catalog.access_sequence = 0;
        state.catalog.hits = 0;
        state.catalog.misses = 0;
        Ok(())
    }

    fn replace_fonts_locked(
        &self,
        state: &mut ProviderState,
        mut fonts: Vec<PackagedFont>,
    ) -> Result<(), MediaError> {
        if !state.in_flight.is_empty() {
            return Err(MediaError::message(
                "ASTRA_TEXT_FONT_GENERATION_BUSY: cannot replace fonts during shaping",
            ));
        }
        let loaded = load_database(&self.context, &self.config, &fonts)?;
        fonts.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
        state.catalog.fonts = fonts;
        state.catalog.database = loaded.database;
        state.catalog.faces = loaded.faces;
        state.catalog.font_systems.clear();
        state.catalog.swash_cache = SwashCache::new();
        state.catalog.layout_cache.clear();
        state.catalog.generation = state.catalog.generation.checked_add(1).ok_or_else(|| {
            MediaError::message("ASTRA_TEXT_FONT_GENERATION: font generation overflow")
        })?;
        state.workers = (0..state.workers.len())
            .map(|_| Arc::new(Mutex::new(font_worker_from_catalog(&state.catalog))))
            .collect();
        Ok(())
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, ProviderState>, MediaError> {
        self.state.lock().map_err(|_| {
            MediaError::message("ASTRA_TEXT_STATE_POISONED: font database lock was poisoned")
        })
    }

    /// Returns the immutable cached layout owned by the font provider.
    ///
    /// Rendering hosts that retain a layout across resource and scene phases
    /// must not deep-clone its shaped runs and glyph resources after Yakui has
    /// already populated the same cache entry during measurement.
    pub fn layout_shared(
        &self,
        request: &TextLayoutRequest,
    ) -> Result<Arc<TextLayoutResult>, MediaError> {
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
        let (cache_key, flight, worker, leader) =
            {
                let mut state = self.lock_state()?;
                validate_family_chain(request, &state.catalog)?;
                let cache_key = request_cache_key(request, &state.catalog.fonts)?;
                state.catalog.access_sequence = state
                    .catalog
                    .access_sequence
                    .checked_add(1)
                    .ok_or_else(|| {
                        MediaError::message("ASTRA_TEXT_CACHE_SEQUENCE: cache sequence overflow")
                    })?;
                let access_sequence = state.catalog.access_sequence;
                if state.catalog.layout_cache.contains_key(&cache_key) {
                    let result =
                        {
                            let entry = state.catalog.layout_cache.get_mut(&cache_key).ok_or_else(
                                || {
                                    MediaError::message(
                                        "ASTRA_TEXT_CACHE_STATE: cached layout disappeared",
                                    )
                                },
                            )?;
                            entry.last_access = access_sequence;
                            Arc::clone(&entry.result)
                        };
                    state.catalog.hits += 1;
                    tracing::trace!(
                        target: "astra_media::text",
                        event = "text.layout.cache_hit",
                        layout_hash = %result.hash,
                        cache_entries = state.catalog.layout_cache.len(),
                    );
                    return Ok(result);
                }
                if let Some(flight) = state.in_flight.get(&cache_key).cloned() {
                    state.catalog.hits += 1;
                    (cache_key, flight, None, false)
                } else {
                    state.catalog.misses += 1;
                    let flight = Arc::new(LayoutFlight::new());
                    state.in_flight.insert(cache_key, Arc::clone(&flight));
                    let worker_index =
                        self.worker_cursor.fetch_add(1, Ordering::Relaxed) % state.workers.len();
                    let worker = Arc::clone(&state.workers[worker_index]);
                    (cache_key, flight, Some(worker), true)
                }
            };
        if !leader {
            return flight.wait();
        }
        let worker = worker.expect("single-flight leader must own a text worker");
        let _budget_lease = WorkerBudgetBroker::global()
            .blocking_acquire()
            .map_err(|error| MediaError::message(error.to_string()))?;
        let active = self.active_workers.fetch_add(1, Ordering::AcqRel) + 1;
        self.peak_active_workers.fetch_max(active, Ordering::AcqRel);
        let _active_guard = ActiveWorkerGuard {
            active: &self.active_workers,
        };
        let layout_result = catch_unwind(AssertUnwindSafe(|| {
            let mut worker = worker.lock().map_err(|_| {
                MediaError::message("ASTRA_TEXT_WORKER_POISONED: font worker lock was poisoned")
            })?;
            layout_uncached(request, &mut worker, &self.context, &self.config).map(Arc::new)
        }))
        .unwrap_or_else(|_| {
            Err(MediaError::message(
                "ASTRA_TEXT_WORKER_PANIC: text layout worker panicked",
            ))
        });
        let result_for_waiters = layout_result
            .as_ref()
            .map(Arc::clone)
            .map_err(ToString::to_string);
        {
            let mut state = self.lock_state()?;
            let registered = state.in_flight.remove(&cache_key).ok_or_else(|| {
                MediaError::message(
                    "ASTRA_TEXT_SINGLE_FLIGHT_STATE: active layout flight disappeared",
                )
            })?;
            if !Arc::ptr_eq(&registered, &flight) {
                return Err(MediaError::message(
                    "ASTRA_TEXT_SINGLE_FLIGHT_STATE: layout flight identity changed",
                ));
            }
            if let Ok(result) = &layout_result {
                insert_cache_entry(
                    &mut state.catalog,
                    cache_key,
                    Arc::clone(result),
                    self.config.max_cache_entries,
                )?;
            }
        }
        flight.complete(result_for_waiters)?;
        let result = layout_result?;
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
}

impl TextLayoutProvider for CosmicTextLayoutProvider {
    fn identity(&self) -> Result<TextLayoutProviderIdentity, MediaError> {
        let state = self.lock_state()?;
        Ok(TextLayoutProviderIdentity {
            context: self.context.clone(),
            fonts: state
                .catalog
                .fonts
                .iter()
                .map(PackagedFontIdentity::from)
                .collect(),
        })
    }

    fn request_hash(&self, request: &TextLayoutRequest) -> Result<Hash256, MediaError> {
        super::validation::validate_request(request, &self.config)?;
        let state = self.lock_state()?;
        validate_family_chain(request, &state.catalog)?;
        request_cache_key(request, &state.catalog.fonts)
    }

    fn layout(&self, request: &TextLayoutRequest) -> Result<TextLayoutResult, MediaError> {
        Ok(self.layout_shared(request)?.as_ref().clone())
    }

    fn measure(&self, request: &TextLayoutRequest) -> Result<TextLayoutMeasurement, MediaError> {
        Ok(TextLayoutMeasurement::from(
            self.layout_shared(request)?.as_ref(),
        ))
    }

    fn layout_hash(&self, request: &TextLayoutRequest) -> Result<Hash256, MediaError> {
        Ok(self.layout(request)?.hash)
    }
}

fn text_worker_count(context: &FontBindingContext) -> usize {
    if !matches!(context.target.as_str(), "windows" | "headless") {
        return 1;
    }
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, 4)
}

fn font_worker_from_catalog(catalog: &FontState) -> FontState {
    FontState {
        fonts: catalog.fonts.clone(),
        database: catalog.database.clone(),
        faces: catalog.faces.clone(),
        font_systems: BTreeMap::new(),
        swash_cache: SwashCache::new(),
        layout_cache: BTreeMap::new(),
        access_sequence: 0,
        generation: catalog.generation,
        hits: 0,
        misses: 0,
    }
}

fn insert_cache_entry(
    catalog: &mut FontState,
    cache_key: Hash256,
    result: Arc<TextLayoutResult>,
    max_cache_entries: usize,
) -> Result<(), MediaError> {
    if catalog.layout_cache.len() == max_cache_entries {
        let oldest = catalog
            .layout_cache
            .iter()
            .min_by_key(|(key, value)| (value.last_access, **key))
            .map(|(key, _)| *key)
            .ok_or_else(|| {
                MediaError::message("ASTRA_TEXT_CACHE_STATE: cache eviction had no candidate")
            })?;
        catalog.layout_cache.remove(&oldest);
    }
    catalog.layout_cache.insert(
        cache_key,
        CacheEntry {
            result,
            last_access: catalog.access_sequence,
        },
    );
    Ok(())
}
