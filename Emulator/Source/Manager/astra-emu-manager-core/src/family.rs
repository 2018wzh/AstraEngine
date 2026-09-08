//! Manager-side family descriptors and probe selection.
//!
//! This module contains the small amount of policy the Manager needs around
//! the independent Family ABI.  It deliberately does not load a dynamic
//! library or own a Family session.  A host adapter converts the ABI DTOs to
//! these records, validates them here, and then hands a selected candidate to
//! the runtime host.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// ABI fingerprint expected by the independent AstraEMU host.
///
/// The Family API crate exports the same value.  Keeping the check in this
/// policy layer means a plugin can never become selectable merely because it
/// has a plausible family ID or a matching file extension.
pub const INDEPENDENT_FAMILY_ABI_FINGERPRINT: &str = "astra.emu.independent_family_abi.v1";
pub const MAX_FAMILY_ID_BYTES: usize = 128;
pub const MAX_GAME_ID_BYTES: usize = 256;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FamilyCapability {
    CpuFrame,
    PcmAudio,
    NativeSave,
    TextReplacement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FamilyPluginDescriptor {
    pub family_id: String,
    pub plugin_id: String,
    pub abi_fingerprint: String,
    pub version: String,
    pub capabilities: Vec<FamilyCapability>,
    pub supported_formats: Vec<String>,
}

impl FamilyPluginDescriptor {
    pub fn validate(&self) -> Result<(), FamilyPolicyError> {
        validate_symbol("family_id", &self.family_id)?;
        validate_symbol("plugin_id", &self.plugin_id)?;
        validate_symbol("version", &self.version)?;
        if self.abi_fingerprint != INDEPENDENT_FAMILY_ABI_FINGERPRINT {
            return Err(FamilyPolicyError::AbiFingerprint);
        }
        if self.capabilities.is_empty() || !self.capabilities.contains(&FamilyCapability::CpuFrame)
        {
            return Err(FamilyPolicyError::MissingCpuFrame);
        }
        for (index, capability) in self.capabilities.iter().enumerate() {
            if self.capabilities[..index].contains(capability) {
                return Err(FamilyPolicyError::DuplicateCapability);
            }
        }
        validate_unique_symbols("supported_formats", &self.supported_formats)?;
        if self.supported_formats.is_empty() {
            return Err(FamilyPolicyError::NoSupportedFormats);
        }
        Ok(())
    }

    pub fn has_capability(&self, capability: FamilyCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FamilyProbeReport {
    pub plugin_id: String,
    pub family_id: String,
    pub game_id: String,
    pub format: String,
    /// Confidence in permyriad, where 10_000 means 100%.
    pub confidence_permyriad: u16,
}

impl FamilyProbeReport {
    pub fn validate(&self) -> Result<(), FamilyPolicyError> {
        validate_symbol("plugin_id", &self.plugin_id)?;
        validate_symbol("family_id", &self.family_id)?;
        validate_bounded_text("game_id", &self.game_id, MAX_GAME_ID_BYTES)?;
        validate_symbol("format", &self.format)?;
        if self.confidence_permyriad > 10_000 {
            return Err(FamilyPolicyError::Confidence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FamilyProbeCandidate {
    pub report: FamilyProbeReport,
    pub descriptor: FamilyPluginDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum FamilyProbeSelection {
    NoMatch,
    Selected(Box<FamilyProbeCandidate>),
    RequiresUserChoice(Vec<FamilyProbeCandidate>),
}

impl FamilyProbeSelection {
    pub fn candidates(&self) -> &[FamilyProbeCandidate] {
        match self {
            Self::NoMatch => &[],
            Self::Selected(candidate) => std::slice::from_ref(candidate.as_ref()),
            Self::RequiresUserChoice(candidates) => candidates,
        }
    }

    pub fn requires_user_choice(&self) -> bool {
        matches!(self, Self::RequiresUserChoice(_))
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FamilyPolicyError {
    #[error("ASTRA_EMU_FAMILY_POLICY_INVALID_SYMBOL: {0}")]
    InvalidSymbol(&'static str),
    #[error("ASTRA_EMU_FAMILY_POLICY_INVALID_GAME_ID")]
    InvalidGameId,
    #[error("ASTRA_EMU_FAMILY_POLICY_ABI_FINGERPRINT")]
    AbiFingerprint,
    #[error("ASTRA_EMU_FAMILY_POLICY_MISSING_CPU_FRAME")]
    MissingCpuFrame,
    #[error("ASTRA_EMU_FAMILY_POLICY_DUPLICATE_CAPABILITY")]
    DuplicateCapability,
    #[error("ASTRA_EMU_FAMILY_POLICY_NO_FORMATS")]
    NoSupportedFormats,
    #[error("ASTRA_EMU_FAMILY_POLICY_DUPLICATE_FORMAT")]
    DuplicateFormat,
    #[error("ASTRA_EMU_FAMILY_POLICY_CONFIDENCE")]
    Confidence,
    #[error("ASTRA_EMU_FAMILY_POLICY_DUPLICATE_PLUGIN")]
    DuplicatePlugin,
    #[error("ASTRA_EMU_FAMILY_POLICY_UNKNOWN_PLUGIN")]
    UnknownPlugin,
    #[error("ASTRA_EMU_FAMILY_POLICY_REPORT_MISMATCH")]
    ReportMismatch,
    #[error("ASTRA_EMU_FAMILY_POLICY_DUPLICATE_REPORT")]
    DuplicateReport,
    #[error("ASTRA_EMU_FAMILY_POLICY_INVALID_PREFERENCE")]
    InvalidPreference,
    #[error("ASTRA_EMU_FAMILY_POLICY_CANDIDATE_NOT_FOUND")]
    CandidateNotFound,
}

/// Registered descriptors in deterministic ID order.
///
/// Registration rejects duplicate plugin IDs.  Probe results are validated
/// against the registered descriptor and all matches are returned; callers
/// must present a chooser whenever more than one result remains.
#[derive(Debug, Default)]
pub struct FamilyPluginRegistry {
    descriptors: BTreeMap<String, FamilyPluginDescriptor>,
}

impl FamilyPluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        descriptor: FamilyPluginDescriptor,
    ) -> Result<(), FamilyPolicyError> {
        descriptor.validate()?;
        if self
            .descriptors
            .insert(descriptor.plugin_id.clone(), descriptor)
            .is_some()
        {
            return Err(FamilyPolicyError::DuplicatePlugin);
        }
        Ok(())
    }

    pub fn remove(&mut self, plugin_id: &str) -> Option<FamilyPluginDescriptor> {
        self.descriptors.remove(plugin_id)
    }

    pub fn descriptor(&self, plugin_id: &str) -> Option<&FamilyPluginDescriptor> {
        self.descriptors.get(plugin_id)
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &FamilyPluginDescriptor> {
        self.descriptors.values()
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }

    pub fn validate_probe(
        &self,
        report: &FamilyProbeReport,
    ) -> Result<FamilyProbeCandidate, FamilyPolicyError> {
        report.validate()?;
        let descriptor = self
            .descriptors
            .get(&report.plugin_id)
            .ok_or(FamilyPolicyError::UnknownPlugin)?;
        if descriptor.family_id != report.family_id
            || !descriptor.supported_formats.contains(&report.format)
        {
            return Err(FamilyPolicyError::ReportMismatch);
        }
        Ok(FamilyProbeCandidate {
            report: report.clone(),
            descriptor: descriptor.clone(),
        })
    }

    /// Validate and order every successful probe.  Ordering is only for a
    /// stable chooser presentation; this method never silently picks the
    /// highest confidence result.
    pub fn select_probe<I>(
        &self,
        reports: I,
        preferred_plugin_id: Option<&str>,
        preferred_family_id: Option<&str>,
    ) -> Result<FamilyProbeSelection, FamilyPolicyError>
    where
        I: IntoIterator<Item = FamilyProbeReport>,
    {
        if preferred_plugin_id.is_some_and(str::is_empty)
            || preferred_family_id.is_some_and(str::is_empty)
        {
            return Err(FamilyPolicyError::InvalidPreference);
        }
        let mut candidates = Vec::new();
        for report in reports {
            let candidate = self.validate_probe(&report)?;
            if candidates.iter().any(|existing: &FamilyProbeCandidate| {
                existing.report.plugin_id == report.plugin_id
            }) {
                return Err(FamilyPolicyError::DuplicateReport);
            }
            if preferred_plugin_id.is_some_and(|id| id != report.plugin_id)
                || preferred_family_id.is_some_and(|id| id != report.family_id)
            {
                continue;
            }
            candidates.push(candidate);
        }
        candidates.sort_by(|left, right| {
            right
                .report
                .confidence_permyriad
                .cmp(&left.report.confidence_permyriad)
                .then_with(|| left.report.plugin_id.cmp(&right.report.plugin_id))
                .then_with(|| left.report.game_id.cmp(&right.report.game_id))
                .then_with(|| left.report.format.cmp(&right.report.format))
        });
        Ok(match candidates.len() {
            0 => FamilyProbeSelection::NoMatch,
            1 => FamilyProbeSelection::Selected(Box::new(candidates.remove(0))),
            _ => FamilyProbeSelection::RequiresUserChoice(candidates),
        })
    }

    pub fn choose_probe(
        &self,
        candidates: &[FamilyProbeCandidate],
        plugin_id: &str,
        game_id: &str,
    ) -> Result<FamilyProbeCandidate, FamilyPolicyError> {
        validate_symbol("plugin_id", plugin_id)?;
        validate_bounded_text("game_id", game_id, MAX_GAME_ID_BYTES)?;
        let mut matches = candidates.iter().filter(|candidate| {
            candidate.report.plugin_id == plugin_id && candidate.report.game_id == game_id
        });
        let candidate = matches.next().ok_or(FamilyPolicyError::CandidateNotFound)?;
        if matches.next().is_some() {
            return Err(FamilyPolicyError::DuplicateReport);
        }
        self.validate_probe(&candidate.report)
    }
}

fn validate_symbol(name: &'static str, value: &str) -> Result<(), FamilyPolicyError> {
    if value.is_empty()
        || value.len() > MAX_FAMILY_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
    {
        return Err(FamilyPolicyError::InvalidSymbol(name));
    }
    Ok(())
}

fn validate_bounded_text(
    name: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), FamilyPolicyError> {
    if value.is_empty() || value.len() > max_bytes || value.contains('\0') {
        return Err(if name == "game_id" {
            FamilyPolicyError::InvalidGameId
        } else {
            FamilyPolicyError::InvalidSymbol(name)
        });
    }
    Ok(())
}

fn validate_unique_symbols(name: &'static str, values: &[String]) -> Result<(), FamilyPolicyError> {
    for (index, value) in values.iter().enumerate() {
        validate_symbol(name, value)?;
        if values[..index].iter().any(|other| other == value) {
            return Err(FamilyPolicyError::DuplicateFormat);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(plugin_id: &str, family_id: &str) -> FamilyPluginDescriptor {
        FamilyPluginDescriptor {
            family_id: family_id.into(),
            plugin_id: plugin_id.into(),
            abi_fingerprint: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
            version: "1.0.0".into(),
            capabilities: vec![FamilyCapability::CpuFrame],
            supported_formats: vec!["fvp.hcb".into()],
        }
    }

    fn report(
        plugin_id: &str,
        family_id: &str,
        game_id: &str,
        confidence: u16,
    ) -> FamilyProbeReport {
        FamilyProbeReport {
            plugin_id: plugin_id.into(),
            family_id: family_id.into(),
            game_id: game_id.into(),
            format: "fvp.hcb".into(),
            confidence_permyriad: confidence,
        }
    }

    #[test]
    fn descriptor_requires_new_abi_and_cpu_frame() {
        let mut value = descriptor("fvp", "fvp");
        value.abi_fingerprint = "astra.emu.family_api.v9".into();
        assert_eq!(value.validate(), Err(FamilyPolicyError::AbiFingerprint));
        value.abi_fingerprint = INDEPENDENT_FAMILY_ABI_FINGERPRINT.into();
        value.capabilities.clear();
        assert_eq!(value.validate(), Err(FamilyPolicyError::MissingCpuFrame));
    }

    #[test]
    fn all_probe_hits_are_returned_for_user_choice() {
        let mut registry = FamilyPluginRegistry::new();
        registry.register(descriptor("fvp-a", "fvp")).unwrap();
        registry.register(descriptor("fvp-b", "fvp")).unwrap();
        let selection = registry
            .select_probe(
                [
                    report("fvp-a", "fvp", "game", 8_000),
                    report("fvp-b", "fvp", "game", 9_000),
                ],
                None,
                None,
            )
            .unwrap();
        let FamilyProbeSelection::RequiresUserChoice(candidates) = selection else {
            panic!("multiple probe hits must require a choice");
        };
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].report.plugin_id, "fvp-b");
        assert_eq!(
            registry
                .choose_probe(&candidates, "fvp-a", "game")
                .unwrap()
                .report
                .plugin_id,
            "fvp-a"
        );
    }

    #[test]
    fn probe_must_match_registered_descriptor() {
        let mut registry = FamilyPluginRegistry::new();
        registry.register(descriptor("fvp", "fvp")).unwrap();
        assert_eq!(
            registry.validate_probe(&report("missing", "fvp", "game", 1)),
            Err(FamilyPolicyError::UnknownPlugin)
        );
        assert_eq!(
            registry.validate_probe(&report("fvp", "other", "game", 1)),
            Err(FamilyPolicyError::ReportMismatch)
        );
    }

    #[test]
    fn explicit_family_preference_filters_candidates() {
        let mut registry = FamilyPluginRegistry::new();
        registry.register(descriptor("fvp", "fvp")).unwrap();
        registry.register(descriptor("other", "other")).unwrap();
        let selection = registry
            .select_probe(
                [
                    report("fvp", "fvp", "game", 1),
                    report("other", "other", "game", 2),
                ],
                None,
                Some("other"),
            )
            .unwrap();
        assert!(matches!(selection, FamilyProbeSelection::Selected(_)));
    }
}
