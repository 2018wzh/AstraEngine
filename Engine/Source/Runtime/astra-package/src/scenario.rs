use std::collections::BTreeSet;

use astra_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ContainerError, PackageReader};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioRefsManifest {
    pub schema: String,
    pub scenarios: Vec<ScenarioReference>,
}

impl ScenarioRefsManifest {
    pub fn empty() -> Self {
        Self {
            schema: "astra.scenario_refs.v2".to_string(),
            scenarios: Vec::new(),
        }
    }

    pub fn validate(&self, reader: &PackageReader) -> Result<(), ContainerError> {
        if self.schema != "astra.scenario_refs.v2" {
            return Err(ContainerError::message(
                "unsupported scenario refs manifest version",
            ));
        }
        let mut paths = BTreeSet::new();
        let mut section_ids = BTreeSet::new();
        for scenario in &self.scenarios {
            validate_relative_path(&scenario.path)?;
            if !paths.insert(scenario.path.as_str()) {
                return Err(ContainerError::message(format!(
                    "duplicate scenario path {}",
                    scenario.path
                )));
            }
            if !section_ids.insert(scenario.section_id.as_str()) {
                return Err(ContainerError::message(format!(
                    "duplicate scenario section binding {}",
                    scenario.section_id
                )));
            }
            let entry = reader
                .container()
                .section_entry(&scenario.section_id)
                .ok_or_else(|| {
                    ContainerError::message(format!(
                        "scenario {} references missing section {}",
                        scenario.path, scenario.section_id
                    ))
                })?;
            if entry.decoded_length != scenario.byte_size || entry.hash != scenario.hash {
                return Err(ContainerError::message(format!(
                    "scenario {} section identity does not match its manifest binding",
                    scenario.path
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PackageBuildRequest, PackageBuilder, SectionPayload};

    fn package_with_manifest(
        manifest: ScenarioRefsManifest,
    ) -> Result<PackageReader, ContainerError> {
        let payload = b"schema: astra.scenario.v1\n".to_vec();
        let mut request = PackageBuildRequest::fixture(
            "com.example.scenario",
            "test",
            vec![SectionPayload::raw(
                "scenario.ref.alpha",
                "astra.scenario.v1",
                payload,
            )],
        );
        request.scenario_refs = serde_json::to_vec(&manifest)
            .map_err(|error| ContainerError::message(error.to_string()))?;
        let package = PackageBuilder::build(request)?;
        PackageReader::open(package.as_bytes())
    }

    fn valid_reference() -> ScenarioReference {
        let payload = b"schema: astra.scenario.v1\n";
        ScenarioReference {
            path: "scenarios/alpha.yaml".to_string(),
            section_id: "scenario.ref.alpha".to_string(),
            hash: Hash256::from_sha256(payload),
            byte_size: payload.len() as u64,
        }
    }

    #[test]
    fn scenario_manifest_binds_normalized_path_to_exact_section_identity() {
        package_with_manifest(ScenarioRefsManifest {
            schema: "astra.scenario_refs.v2".to_string(),
            scenarios: vec![valid_reference()],
        })
        .unwrap();

        for invalid in [
            ScenarioReference {
                path: "../alpha.yaml".to_string(),
                ..valid_reference()
            },
            ScenarioReference {
                section_id: "scenario.ref.missing".to_string(),
                ..valid_reference()
            },
            ScenarioReference {
                hash: Hash256::from_sha256(b"different"),
                ..valid_reference()
            },
        ] {
            assert!(package_with_manifest(ScenarioRefsManifest {
                schema: "astra.scenario_refs.v2".to_string(),
                scenarios: vec![invalid],
            })
            .is_err());
        }
    }

    #[test]
    fn scenario_manifest_rejects_duplicate_path_or_section_authority() {
        let duplicate = valid_reference();
        let mut duplicate_path = duplicate.clone();
        duplicate_path.section_id = "scenario.ref.other".to_string();
        let mut duplicate_section = duplicate.clone();
        duplicate_section.path = "scenarios/other.yaml".to_string();
        for refs in [
            vec![duplicate.clone(), duplicate_path],
            vec![duplicate.clone(), duplicate_section],
        ] {
            assert!(package_with_manifest(ScenarioRefsManifest {
                schema: "astra.scenario_refs.v2".to_string(),
                scenarios: refs,
            })
            .is_err());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioReference {
    pub path: String,
    pub section_id: String,
    pub hash: Hash256,
    pub byte_size: u64,
}

fn validate_relative_path(path: &str) -> Result<(), ContainerError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(ContainerError::message(format!(
            "scenario path is not a normalized relative path: {path}"
        )));
    }
    Ok(())
}
