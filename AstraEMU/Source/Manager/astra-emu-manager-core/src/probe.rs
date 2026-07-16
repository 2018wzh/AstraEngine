use std::{collections::BTreeMap, sync::Arc};

use astra_emu_family_api::{
    LegacyProbeReport, LegacyProbeRequest, LegacyRuntimeHostCtx, LegacyRuntimeProvider,
};
use thiserror::Error;

pub const DEFAULT_PROBE_ORDER: [&str; 7] = [
    "krkr", "artemis", "bgi", "siglus", "softpal", "fvp", "minori",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeBinding {
    pub family_id: String,
    pub provider_id: String,
    pub report: LegacyProbeReport,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProbeError {
    #[error("ASTRA_EMU_PROBE_EXPLICIT_PROVIDER_MISSING: {0}")]
    ExplicitProviderMissing(String),
    #[error("ASTRA_EMU_PROBE_DUPLICATE_PROVIDER: {0}")]
    DuplicateProvider(String),
    #[error("ASTRA_EMU_PROBE_AMBIGUOUS: {0}")]
    Ambiguous(String),
    #[error("ASTRA_EMU_PROBE_NO_MATCH")]
    NoMatch,
    #[error("ASTRA_EMU_PROBE_PROVIDER: {0}")]
    Provider(String),
}

pub struct AutoProbe {
    providers: BTreeMap<String, Arc<dyn LegacyRuntimeProvider>>,
}

impl AutoProbe {
    pub fn new(
        providers: impl IntoIterator<Item = Arc<dyn LegacyRuntimeProvider>>,
    ) -> Result<Self, ProbeError> {
        let mut map = BTreeMap::new();
        for provider in providers {
            let descriptor = provider.descriptor();
            descriptor
                .validate()
                .map_err(|error| ProbeError::Provider(error.to_string()))?;
            if map
                .insert(descriptor.family_id.0.clone(), provider)
                .is_some()
            {
                return Err(ProbeError::DuplicateProvider(descriptor.family_id.0));
            }
        }
        Ok(Self { providers: map })
    }

    pub fn select(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: &LegacyProbeRequest,
        explicit_family: Option<&str>,
    ) -> Result<ProbeBinding, ProbeError> {
        if let Some(family) = explicit_family {
            let provider = self
                .providers
                .get(family)
                .ok_or_else(|| ProbeError::ExplicitProviderMissing(family.to_owned()))?;
            return self.probe_one(provider, ctx, request);
        }

        let mut best: Option<ProbeBinding> = None;
        for family in DEFAULT_PROBE_ORDER {
            let Some(provider) = self.providers.get(family) else {
                continue;
            };
            let candidate = self.probe_one(provider, ctx, request)?;
            if candidate.report.confidence_permyriad == 0 || !candidate.report.blockers.is_empty() {
                continue;
            }
            match &best {
                None => best = Some(candidate),
                Some(current)
                    if candidate.report.confidence_permyriad
                        > current.report.confidence_permyriad =>
                {
                    best = Some(candidate)
                }
                Some(current)
                    if candidate.report.confidence_permyriad
                        == current.report.confidence_permyriad =>
                {
                    return Err(ProbeError::Ambiguous(format!(
                        "{} and {} reported confidence {}",
                        current.family_id,
                        candidate.family_id,
                        candidate.report.confidence_permyriad
                    )));
                }
                Some(_) => {}
            }
        }
        best.ok_or(ProbeError::NoMatch)
    }

    fn probe_one(
        &self,
        provider: &Arc<dyn LegacyRuntimeProvider>,
        ctx: &LegacyRuntimeHostCtx,
        request: &LegacyProbeRequest,
    ) -> Result<ProbeBinding, ProbeError> {
        let descriptor = provider.descriptor();
        descriptor
            .validate()
            .map_err(|error| ProbeError::Provider(error.to_string()))?;
        let report = provider
            .probe(ctx, request.clone())
            .map_err(|error| ProbeError::Provider(error.to_string()))?;
        report
            .validate()
            .map_err(|error| ProbeError::Provider(error.to_string()))?;
        Ok(ProbeBinding {
            family_id: descriptor.family_id.0,
            provider_id: descriptor.provider_id,
            report,
        })
    }
}
