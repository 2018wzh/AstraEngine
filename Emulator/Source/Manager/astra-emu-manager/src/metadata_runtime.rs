use std::{sync::mpsc, thread, time::Duration};

use astra_emu_metadata::{
    BangumiPlayUpdate, BangumiProvider, BangumiProviderConfig, CoverAsset, MetadataLicenseManifest,
    MetadataProvider, MetadataProviderId, MetadataRecord, MetadataSearchQuery, ReleaseUse,
    VndbProvider, VndbProviderConfig,
};

const COMMAND_CAPACITY: usize = 64;

#[derive(Debug)]
pub enum MetadataCommandKind {
    Search(MetadataSearchQuery),
    Fetch(String),
    SyncBangumi(BangumiPlayUpdate),
}

#[derive(Debug)]
pub struct MetadataCommand {
    pub request_id: String,
    pub case_identity: String,
    pub provider: MetadataProviderId,
    pub access_token: Option<String>,
    pub allow_sensitive_cover: bool,
    pub kind: MetadataCommandKind,
}

#[derive(Debug)]
pub enum MetadataPayload {
    Search(Vec<MetadataRecord>),
    Fetch {
        record: Box<MetadataRecord>,
        cover: Option<CoverAsset>,
    },
    BangumiPlaySynced,
}

#[derive(Debug)]
pub struct MetadataCompletion {
    pub request_id: String,
    pub case_identity: String,
    pub provider: MetadataProviderId,
    pub result: Result<MetadataPayload, String>,
}

pub struct MetadataRuntime {
    commands: mpsc::SyncSender<MetadataCommand>,
    completions: mpsc::Receiver<MetadataCompletion>,
}

impl MetadataRuntime {
    pub fn start() -> Result<Self, String> {
        let (commands, command_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (completion_tx, completions) = mpsc::channel();
        thread::Builder::new()
            .name("astra-emu-metadata".into())
            .spawn(move || worker(command_rx, completion_tx))
            .map_err(|_| "ASTRA_EMU_METADATA_WORKER_START".to_owned())?;
        Ok(Self {
            commands,
            completions,
        })
    }

    pub fn submit(&self, command: MetadataCommand) -> Result<(), String> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "ASTRA_EMU_METADATA_QUEUE_FULL".into(),
                mpsc::TrySendError::Disconnected(_) => {
                    "ASTRA_EMU_METADATA_WORKER_DISCONNECTED".into()
                }
            })
    }

    pub fn try_recv(&self) -> Result<Option<MetadataCompletion>, String> {
        match self.completions.try_recv() {
            Ok(completion) => Ok(Some(completion)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("ASTRA_EMU_METADATA_WORKER_DISCONNECTED".into())
            }
        }
    }
}

fn worker(
    commands: mpsc::Receiver<MetadataCommand>,
    completions: mpsc::Sender<MetadataCompletion>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return,
    };
    while let Ok(command) = commands.recv() {
        let result = runtime.block_on(execute(&command));
        if completions
            .send(MetadataCompletion {
                request_id: command.request_id,
                case_identity: command.case_identity,
                provider: command.provider,
                result,
            })
            .is_err()
        {
            break;
        }
    }
}

async fn execute(command: &MetadataCommand) -> Result<MetadataPayload, String> {
    match command.provider {
        MetadataProviderId::Vndb => {
            let provider = VndbProvider::new(VndbProviderConfig {
                network_consent: true,
                license: MetadataLicenseManifest {
                    release_use: ReleaseUse::NonCommercial,
                    vndb_commercial_license_id: None,
                },
                timeout: Duration::from_secs(20),
                minimum_request_delay: Duration::from_millis(1_000),
            })
            .map_err(|error| error.to_string())?;
            execute_provider(&provider, command).await
        }
        MetadataProviderId::Bangumi => {
            let provider = BangumiProvider::new(BangumiProviderConfig {
                network_consent: true,
                access_token: command.access_token.clone(),
                timeout: Duration::from_secs(20),
            })
            .map_err(|error| error.to_string())?;
            if let MetadataCommandKind::SyncBangumi(update) = &command.kind {
                provider
                    .sync_play_status(update)
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(MetadataPayload::BangumiPlaySynced);
            }
            execute_provider(&provider, command).await
        }
    }
}

async fn execute_provider(
    provider: &impl MetadataProvider,
    command: &MetadataCommand,
) -> Result<MetadataPayload, String> {
    match &command.kind {
        MetadataCommandKind::Search(query) => provider
            .search(query)
            .await
            .map(MetadataPayload::Search)
            .map_err(|error| error.to_string()),
        MetadataCommandKind::Fetch(remote_id) => {
            let record = provider
                .fetch_by_id(remote_id)
                .await
                .map_err(|error| error.to_string())?;
            let cover =
                if record.cover.is_some() && (!record.sensitive || command.allow_sensitive_cover) {
                    Some(
                        provider
                            .fetch_cover(&record, command.allow_sensitive_cover)
                            .await
                            .map_err(|error| error.to_string())?,
                    )
                } else {
                    None
                };
            Ok(MetadataPayload::Fetch {
                record: Box::new(record),
                cover,
            })
        }
        MetadataCommandKind::SyncBangumi(_) => Err("ASTRA_EMU_METADATA_PROVIDER_MISMATCH".into()),
    }
}
