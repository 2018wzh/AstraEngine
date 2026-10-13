use super::stage::*;
use super::ui_layout::*;
use super::*;

impl NativeVnHostCommandSource {
    pub fn from_package(
        package: &astra_package::PackageReader,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
    ) -> Result<Self, NativeVnHostError> {
        Self::from_package_with_execution(
            package,
            config,
            width,
            height,
            surface,
            NativeVnRuntimeExecution::shipping_serial(),
        )
    }

    pub fn from_package_with_execution(
        package: &astra_package::PackageReader,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
        runtime_execution: NativeVnRuntimeExecution,
    ) -> Result<Self, NativeVnHostError> {
        let budget = NativeVnDecodedCacheBudget::partition(DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES)?;
        Self::from_package_with_options(
            package,
            config,
            width,
            height,
            surface,
            NativeVnHostOpenOptions {
                max_asset_cache_bytes: budget.asset_bytes,
                max_glyph_cache_bytes: budget.glyph_bytes,
                runtime_execution,
                policy_actions: crate::PreparedPolicyActions::load(
                    package,
                    budget.asset_bytes,
                    &astra_runtime::TaskScope::new(),
                )?,
            },
        )
    }

    pub fn from_package_with_options(
        package: &astra_package::PackageReader,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
        options: NativeVnHostOpenOptions,
    ) -> Result<Self, NativeVnHostError> {
        let NativeVnHostOpenOptions {
            max_asset_cache_bytes,
            max_glyph_cache_bytes,
            runtime_execution,
            policy_actions,
        } = options;
        runtime_execution.validate()?;
        if policy_actions.package_hash != package.package_hash() {
            return Err(NativeVnHostError::Package(
                "ASTRA_PLAYER_POLICY_PACKAGE_IDENTITY".into(),
            ));
        }
        let max_asset_cache_bytes = max_asset_cache_bytes
            .checked_sub(policy_actions.byte_len)
            .filter(|remaining| *remaining > 0)
            .ok_or_else(|| NativeVnHostError::Asset("ASTRA_PLAYER_POLICY_CACHE_BUDGET".into()))?;
        validate_product_provider_bindings(package)?;
        let product_context = package.product_context().clone();
        if config.profile != product_context.profile() {
            return Err(NativeVnHostError::Package(format!(
                "ASTRA_PLAYER_RUNTIME_PROFILE_MISMATCH: requested profile {} does not match package provider profile {}",
                config.profile,
                product_context.profile()
            )));
        }
        let compiled = decode_compiled_project(package)
            .map_err(|err| NativeVnHostError::Package(err.to_string()))?;
        let system_ui_manifest = package
            .container()
            .decode_postcard::<VnSystemUiProfileManifest>("vn.system_ui_profile_manifest")
            .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        let validation = system_ui_manifest.validate();
        if validation.status != astra_vn_core::SystemStoryValidationStatus::Pass {
            return Err(NativeVnHostError::Package(
                "ASTRA_PLAYER_SYSTEM_UI_PROFILE_BLOCKED: package system UI policy is invalid"
                    .into(),
            ));
        }
        let system_ui_policy = system_ui_manifest
            .profiles
            .get(&config.profile)
            .cloned()
            .ok_or_else(|| {
                NativeVnHostError::Package(format!(
                    "ASTRA_PLAYER_SYSTEM_UI_PROFILE_MISSING: profile {} has no declared policy",
                    config.profile
                ))
            })?;
        let asset_store = PackageAssetStore::index(package, max_asset_cache_bytes)?;
        let presentation_manifest =
            load_presentation_provider_manifest(package, &config.profile)
                .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        validate_story_presentation(
            &compiled,
            &asset_store,
            &presentation_manifest,
            &config.profile,
        )?;
        let locale_config = load_player_locale_config(package)
            .map_err(|error| NativeVnHostError::Localization(error.to_string()))?;
        if locale_config
            .available_locales
            .binary_search(&config.locale)
            .is_err()
        {
            return Err(NativeVnHostError::Localization(format!(
                "ASTRA_PLAYER_LOCALE_UNDECLARED: locale {} is not declared by player.locale_config",
                config.locale
            )));
        }
        let localizations = locale_config
            .available_locales
            .iter()
            .map(|locale| {
                load_package_localization(package, locale, 16 * 1024 * 1024)
                    .map(|table| (locale.clone(), table))
                    .map_err(|error| NativeVnHostError::Localization(error.to_string()))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let localization = localizations.get(&config.locale).cloned().ok_or_else(|| {
            NativeVnHostError::Localization(
                "ASTRA_PLAYER_LOCALE_TABLE_MISSING: declared locale has no loaded table".into(),
            )
        })?;
        let text_provider = Arc::new(astra_media::text_layout_from_package(
            package,
            "media.font_manifest",
            FontBindingContext {
                target: product_context.target().to_string(),
                profile: config.profile.clone(),
                default_locale: config.locale.clone(),
            },
            TextLayoutConfig::production_defaults(),
        )?);
        let font_families = text_provider
            .identity()?
            .fonts
            .into_iter()
            .map(|font| font.family)
            .collect::<Vec<_>>();
        validate_story_text(&compiled, &localization, &text_provider, &font_families)?;
        text_provider.clear_layout_cache()?;
        let max_glyph_cache_bytes = usize::try_from(max_glyph_cache_bytes).map_err(|_| {
            NativeVnHostError::Asset("ASTRA_PLAYER_GLYPH_CACHE_BUDGET_PLATFORM_OVERFLOW".into())
        })?;
        Self::open(
            compiled,
            config,
            width,
            height,
            surface,
            ProductPackageBinding {
                product_context,
                package_hash: package.package_hash(),
                presentation: ProductPresentationBinding {
                    asset_store,
                    localization,
                    localizations,
                    text_provider,
                    font_families,
                    manifest: presentation_manifest,
                },
                system_ui_policy,
                policy_actions,
            },
            NativeVnHostRuntimeOptions {
                cache_budget: NativeVnHostCacheBudget {
                    asset_bytes: max_asset_cache_bytes,
                    glyph_bytes: max_glyph_cache_bytes,
                },
                execution: runtime_execution,
            },
        )
    }

    pub(super) fn open(
        compiled: CompiledVnProject,
        config: VnRunConfig,
        width: u32,
        height: u32,
        surface: PlayerHostResourceId,
        binding: ProductPackageBinding,
        runtime_options: NativeVnHostRuntimeOptions,
    ) -> Result<Self, NativeVnHostError> {
        let NativeVnHostRuntimeOptions {
            cache_budget,
            execution: runtime_execution,
        } = runtime_options;
        if compiled.story.story_manifest.stories.is_empty() {
            return Err(NativeVnHostError::EmptyStory);
        }
        let terminal_routes = compiled
            .story
            .route_graph
            .nodes
            .iter()
            .filter(|node| node.terminal)
            .map(|node| node.id.clone())
            .collect();
        let stage_director = ProductStageDirector::new(
            binding.presentation.manifest.clone(),
            binding.product_context.profile(),
            astra_vn_package::StageViewport { width, height },
        )
        .map_err(stage_director_error)?;
        let product_context = &binding.product_context;
        let ui_profile = config.profile.clone();
        if compiled.themes.is_empty() {
            return Err(NativeVnHostError::Package(
                "ASTRA_PLAYER_UI_THEME_MISSING: compiled project has no packaged UI theme".into(),
            ));
        }
        let ui_text_locale = Arc::new(RwLock::new(
            binding.presentation.localization.locale.clone(),
        ));
        let ordered_font_families = ordered_ui_font_families(
            &binding.presentation.font_families,
            &binding.presentation.localization.locale,
        );
        let ui_text_font_families = Arc::new(RwLock::new(ordered_font_families.clone()));
        let ui_text_measurer = Arc::new(NativeUiTextMeasurer {
            provider: Arc::clone(&binding.presentation.text_provider),
            font_families: Arc::clone(&ui_text_font_families),
            locale: Arc::clone(&ui_text_locale),
            frame_layouts: Mutex::new(BTreeMap::new()),
        });
        let ui_renderer = BlueprintYakuiRenderer::new(compiled.ui_blueprints.clone())?
            .with_image_resource_provider(binding.presentation.asset_store.clone())
            .with_text_measurer(ui_text_measurer.clone());
        let ui_backend = AstraYakuiBackend::new(ui_renderer, compiled.project_hash)?;
        let mut ui_controller_host = LuauUiControllerHost::with_default_budget()
            .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        let mut unique_controller_sources = compiled
            .controller_sources
            .values()
            .cloned()
            .collect::<BTreeSet<_>>();
        for source in std::mem::take(&mut unique_controller_sources) {
            ui_controller_host
                .register_source(source)
                .map_err(|error| NativeVnHostError::Package(error.to_string()))?;
        }
        let registered_controller_ids = ui_controller_host
            .manifests()
            .map(|manifest| manifest.id.clone())
            .collect::<BTreeSet<_>>();
        if registered_controller_ids != compiled.controller_ids {
            return Err(NativeVnHostError::Package(
                "ASTRA_PLAYER_UI_CONTROLLER_SET: packaged controller source set does not match bindings"
                    .into(),
            ));
        }
        let limits = VnHostLimits::new();
        let mut host = NativeVnRuntimeHost::new(product_context, limits)?;
        let story = Arc::new(compiled.story);
        let open = match host.open(
            Arc::clone(&story),
            config,
            astra_vn::VnSessionConfig {
                target_id: product_context.target().into(),
                seed: 0,
                package: Some(astra_runtime::PackageHandle {
                    package_id: binding.package_hash.to_string(),
                    target: product_context.target().into(),
                    ..Default::default()
                }),
                integrity_mode: runtime_execution.integrity_mode,
                worker_count: runtime_execution.worker_count,
            },
        ) {
            Ok(session_id) => session_id,
            Err(error) => return Err(cleanup_runtime_host(&mut host, error)),
        };
        host.bind_policy(binding.policy_actions)?;
        tracing::info!(
            event = "player.vn.runtime.open",
            width,
            height,
            "opened AstraVN Player command source through its owned NativeVN runtime"
        );
        let localization_keys = binding
            .presentation
            .localization
            .strings
            .keys()
            .cloned()
            .collect();
        let ui_view_localization_keys = blueprint_view_localization_keys(&compiled.ui_blueprints);
        let image_prefetch_windows =
            image_prefetch_windows(&story, &binding.presentation.asset_store)?;
        let image_prefetcher =
            PackageImagePrefetcher::start(Arc::clone(&binding.presentation.asset_store))?;
        Ok(Self {
            host,
            session_id: open,
            runtime_state: None,
            runtime_backlog_count: 0,
            font_families: ordered_font_families,
            available_font_families: binding.presentation.font_families,
            text_resources: TextRenderResourceOwner::with_retained_glyph_cache(
                8_192,
                cache_budget.glyph_bytes,
            )?,
            localization: binding.presentation.localization,
            localization_keys,
            localizations: binding.presentation.localizations,
            ui_text_locale,
            ui_text_font_families,
            ui_text_measurer,
            surface,
            command_sequence: 0,
            fixed_step: 0,
            session_seed: 0,
            next_step_mode: astra_runtime::TickMode::Live,
            width,
            height,
            output_extent: astra_media_core::Extent2D { width, height },
            ui_viewport: UiViewport {
                physical_width: width,
                physical_height: height,
                scale_factor: 1.0,
                font_scale: 1.0,
                safe_area_points: UiInsets {
                    left: 0.0,
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                },
            },
            textures: BTreeMap::new(),
            texture_dimensions: BTreeMap::new(),
            asset_store: binding.presentation.asset_store,
            image_prefetcher,
            image_prefetch_windows,
            image_prefetch_inflight: BTreeSet::new(),
            image_prefetch_failure: None,
            live_texture_ids: BTreeSet::new(),
            gpu_resources_invalidated: false,
            live_texture_bytes: BTreeMap::new(),
            pending_gpu_lifecycle: Vec::new(),
            texture_last_used: BTreeMap::new(),
            texture_cpu_last_used: BTreeMap::new(),
            texture_use_clock: 0,
            texture_cpu_bytes: 0,
            texture_cpu_budget_bytes: cache_budget.asset_bytes,
            resident_texture_bytes: 0,
            gpu_texture_budget_bytes: cache_budget
                .asset_bytes
                .min(DEFAULT_NATIVE_VN_GPU_TEXTURE_CACHE_BYTES),
            live_layout_ids: BTreeSet::new(),
            scene_draw: Vec::new(),
            director_transition_snapshot: None,
            last_step_evidence: None,
            terminal_routes,
            pending_timeline: Vec::new(),
            pending_audio: Vec::new(),
            pending_audio_preloads: Vec::new(),
            audio_preload_story_ids: BTreeSet::new(),
            pending_video: Vec::new(),
            pending_stage_completions: Vec::new(),
            next_media_resource_id: 10_000,
            stage_director,
            presentation_failed: false,
            compiled_project_hash: compiled.project_hash,
            media_scope: astra_runtime::TaskScope::new(),
            video_scopes: BTreeMap::new(),
            story,
            ui_blueprints: compiled.ui_blueprints,
            ui_view_localization_keys,
            ui_bindings: compiled.ui_bindings,
            ui_backend,
            ui_backend_started: false,
            ui_themes: compiled
                .themes
                .into_iter()
                .map(|(id, theme)| (id, Arc::new(theme)))
                .collect(),
            ui_profile,
            system_ui_policy: binding.system_ui_policy.clone(),
            ui_generation: 1,
            ui_input_sequence: 0,
            ui_draw: Vec::new(),
            ui_semantics: None,
            ui_save_slots: save_slots_for_policy(&binding.system_ui_policy),
            pending_ui_host_request: None,
            gameplay_thumbnail_capture: None,
            pending_save_metadata: None,
            pending_save_completion: None,
            exit_requested: false,
            ui_controller_host,
            ui_controller_sessions: BTreeMap::new(),
            base_ui_instance_id: None,
            base_ui_theme_id: None,
            active_ui_controller: None,
            ui_modals: Vec::new(),
            pending_ui_focus: None,
            ui_animations: BTreeMap::new(),
            ui_performance: UiPerformanceGate::new(UiPerformanceBudget::production()),
            last_ui_performance_sample: None,
            ui_frame_reuse: None,
            ui_host_performance_sampling_enabled: false,
            last_ui_host_performance_sample: None,
            shutdown_started: false,
        })
    }
}
