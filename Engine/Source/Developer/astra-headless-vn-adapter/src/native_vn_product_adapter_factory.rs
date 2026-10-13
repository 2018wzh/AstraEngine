use super::*;

#[derive(Default)]
pub struct NativeVnProductAdapterFactory {
    pub(super) package_crypto: Option<Arc<dyn ContainerCryptoProvider>>,
}

impl NativeVnProductAdapterFactory {
    pub fn with_package_crypto(package_crypto: Arc<dyn ContainerCryptoProvider>) -> Self {
        Self {
            package_crypto: Some(package_crypto),
        }
    }
}

impl ProductAdapterFactory for NativeVnProductAdapterFactory {
    fn binding_id(&self) -> &str {
        "astra.native_vn"
    }

    fn open<'a>(
        &'a self,
        request: ProductOpenRequest,
    ) -> ProductFuture<'a, Result<Box<dyn ProductSession>, ProductHostError>> {
        let package_crypto = self.package_crypto.clone();
        Box::pin(async move {
            let performance_observer = request.performance_observer.clone();
            if request.presentation_rate_hz < HEADLESS_PRESENTATION_RATE_HZ
                || !request
                    .presentation_rate_hz
                    .is_multiple_of(HEADLESS_PRESENTATION_RATE_HZ)
            {
                return Err(ProductHostError::Binding(
                    "presentation rate must be an integer multiple of the authoritative Runtime tick rate"
                        .into(),
                ));
            }
            let presentation_substeps =
                request.presentation_rate_hz / HEADLESS_PRESENTATION_RATE_HZ;
            tracing::info!(
                event = "headless.product.native_vn.open",
                target = %request.target,
                profile = %request.profile,
                "opening NativeVN through the generic Headless product host"
            );
            let raw = match request.package {
                ProductPackageSource::InMemory(bytes) => AstraContainerReader::new(&bytes),
                ProductPackageSource::VerifiedContainer(container) => Ok(container),
                ProductPackageSource::StorageVerified {
                    source,
                    storage_hash,
                } => AstraContainerReader::open_storage_verified_source(source, storage_hash),
            }
            .map_err(|error| binding("package.inspect", error))?;
            let source_locked = raw.has_section("source.unlock");
            let package = match (source_locked, package_crypto) {
                (false, None) => PackageReader::open_verified_container(raw),
                (false, Some(_)) => {
                    return Err(ProductHostError::Binding(
                        "package.unlock: plaintext package received unexpected crypto provider"
                            .into(),
                    ));
                }
                (true, None) => {
                    return Err(ProductHostError::Binding(
                        "package.unlock: source-locked package requires verified source input"
                            .into(),
                    ));
                }
                (true, Some(crypto)) => {
                    let policy: SourceUnlockPolicy = raw
                        .decode_postcard("source.unlock")
                        .map_err(|error| binding("package.unlock_policy", error))?;
                    PackageReader::open_source_locked_container(
                        raw,
                        &policy,
                        "source.unlock",
                        crypto,
                    )
                }
            }
            .map_err(|error| binding("package.open", error))?;
            record_open_phase(&performance_observer, "product.package_opened")?;
            let locale = match request.locale {
                Some(locale) => locale,
                None => {
                    astra_vn_package::load_player_locale_config(&package)
                        .map_err(|error| binding("locale.load", error))?
                        .default_locale
                }
            };
            record_open_phase(&performance_observer, "product.locale_loaded")?;
            let window = request
                .platform
                .create_window(WindowRequest {
                    title: "Astra Headless Product".into(),
                    width: request.width,
                    height: request.height,
                    visible: false,
                })
                .await
                .map_err(|error| binding("window.create", error))?;
            record_open_phase(&performance_observer, "product.window_created")?;
            let surface = request
                .platform
                .create_surface(SurfaceRequest {
                    window,
                    width: request.width,
                    height: request.height,
                })
                .await
                .map_err(|error| binding("surface.create", error))?;
            record_open_phase(&performance_observer, "product.surface_created")?;
            let logical_surface = PlayerHostResourceId(1);
            let cache_budget =
                NativeVnDecodedCacheBudget::partition(request.max_decoded_cache_bytes)
                    .map_err(|error| ProductHostError::Binding(error.to_string()))?;
            let asset_cache_bytes = cache_budget.asset_bytes;
            let audio_cache_bytes = cache_budget.audio_bytes;
            let glyph_cache_bytes = cache_budget.glyph_bytes;
            let mut sink = PlatformCommandSink::new(request.platform.clone());
            sink.bind_surface(logical_surface, surface)
                .map_err(|error| binding("surface.bind", error))?;
            let mut executor = PlayerHostCommandExecutor::new(sink);
            let mut source = NativeVnHostCommandSource::from_package_with_options(
                &package,
                VnRunConfig {
                    profile: request.profile,
                    locale,
                },
                request.width,
                request.height,
                logical_surface,
                astra_player_vn::NativeVnHostOpenOptions {
                    policy_actions: astra_player_vn::PreparedPolicyActions::load(
                        &package,
                        asset_cache_bytes,
                        &astra_runtime::TaskScope::new(),
                    )
                    .map_err(|error| binding("policy.preload", error))?,
                    max_asset_cache_bytes: asset_cache_bytes,
                    max_glyph_cache_bytes: glyph_cache_bytes,
                    runtime_execution: astra_player_vn::NativeVnRuntimeExecution::evidence_parallel(
                    ),
                },
            )
            .map_err(|error| binding("runtime.open", error))?;
            source.set_ui_host_performance_sampling_enabled(performance_observer.is_some());
            record_open_phase(&performance_observer, "product.runtime_opened")?;
            hydrate_save_catalog(&mut source, &mut executor).await?;
            record_open_phase(&performance_observer, "product.save_catalog_hydrated")?;
            executor
                .execute_batch(
                    source
                        .launch()
                        .map_err(|error| binding("runtime.launch", error))?,
                )
                .await
                .map_err(|error| binding("host.launch", error))?;
            record_open_phase(&performance_observer, "product.runtime_launched")?;
            let mut media = NativeVnProductMediaHost::with_video_limits(
                256,
                request.max_video_frames,
                request.max_decode_output_bytes,
                audio_cache_bytes,
                request.retain_audio_timeline,
            )
            .map_err(|error| binding("media.create", error))?;
            media.set_performance_profiling(performance_observer.is_some());
            record_open_phase(&performance_observer, "product.media_created")?;
            media
                .initialize(&mut source, &mut executor)
                .await
                .map_err(|error| binding("media.initialize", error))?;
            record_open_phase(&performance_observer, "product.media_initialized")?;
            media
                .process(&mut source, &mut executor, 0, Vec::new())
                .await
                .map_err(|error| binding("media.launch", error))?;
            record_open_phase(&performance_observer, "product.media_launched")?;
            Ok(Box::new(NativeVnHeadlessSession {
                platform: request.platform,
                window,
                surface,
                source: Some(source),
                executor,
                media,
                pointer: (0.0, 0.0),
                viewport: (request.width, request.height),
                next_save_transaction: 1_000,
                observations: Vec::new(),
                resumed: false,
                focused: false,
                last_tick: 0,
                presentation_substeps,
                performance_observer: performance_observer.clone(),
                performance: performance_observer
                    .is_some()
                    .then(ProductPerformanceSample::default),
            }) as Box<dyn ProductSession>)
        })
    }
}

pub(super) fn record_open_phase(
    observer: &Option<Arc<dyn ProductPerformanceObserver>>,
    name: &str,
) -> Result<(), ProductHostError> {
    observer
        .as_ref()
        .map(|observer| observer.record_phase(name))
        .transpose()
        .map_err(ProductHostError::Output)?;
    Ok(())
}

pub(super) async fn hydrate_save_catalog(
    source: &mut NativeVnHostCommandSource,
    executor: &mut PlayerHostCommandExecutor<PlatformCommandSink>,
) -> Result<(), ProductHostError> {
    let results = executor
        .execute_batch(
            source
                .list_saves()
                .map_err(|error| binding("save.list.prepare", error))?,
        )
        .await
        .map_err(|error| binding("save.list", error))?;
    let slots = match results.as_slice() {
        [PlayerHostCommandResult::SaveList { slots }] => slots.clone(),
        _ => {
            return Err(ProductHostError::Binding(
                "ASTRA_HEADLESS_SAVE_LIST_RESULT_INVALID".into(),
            ));
        }
    };
    for slot in &slots {
        let results = executor
            .execute_batch(
                source
                    .read_save(slot)
                    .map_err(|error| binding("save.catalog.read.prepare", error))?,
            )
            .await
            .map_err(|error| binding("save.catalog.read", error))?;
        let bytes = match results.as_slice() {
            [PlayerHostCommandResult::SaveRead { bytes }] => bytes,
            _ => {
                return Err(ProductHostError::Binding(
                    "ASTRA_HEADLESS_SAVE_CATALOG_RESULT_INVALID".into(),
                ));
            }
        };
        source
            .ingest_save_catalog_entry(slot, bytes)
            .map_err(|error| binding("save.catalog.ingest", error))?;
    }
    tracing::info!(
        event = "headless.product.native_vn.save_catalog_hydrated",
        slot_count = slots.len(),
        "hydrated validated save metadata before launching NativeVN"
    );
    Ok(())
}

pub(super) struct NativeVnHeadlessSession {
    pub(super) platform: astra_platform::PlatformHostClient,
    pub(super) window: WindowHandle,
    pub(super) surface: SurfaceHandle,
    pub(super) source: Option<NativeVnHostCommandSource>,
    pub(super) executor: PlayerHostCommandExecutor<PlatformCommandSink>,
    pub(super) media: NativeVnProductMediaHost,
    pub(super) pointer: (f64, f64),
    pub(super) viewport: (u32, u32),
    pub(super) next_save_transaction: u64,
    pub(super) observations: Vec<Observation>,
    pub(super) resumed: bool,
    pub(super) focused: bool,
    pub(super) last_tick: u64,
    pub(super) presentation_substeps: u32,
    pub(super) performance_observer: Option<Arc<dyn ProductPerformanceObserver>>,
    pub(super) performance: Option<ProductPerformanceSample>,
}
