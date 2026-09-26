use astra_package::{ContainerError, PackageReader, SectionPayload};
use astra_vn_policy::{PolicyActionBinding, POLICY_ACTION_SECTION};
use std::collections::{BTreeMap, BTreeSet};

pub const POLICY_ACTIONS_SCHEMA: &str = "astra.vn.policy_actions.v1";
const MAX_ACTION_SECTION_BYTES: usize = 4 * 1024 * 1024;

pub fn policy_action_section(
    bindings: &[PolicyActionBinding],
) -> Result<SectionPayload, ContainerError> {
    validate(bindings)?;
    let section =
        SectionPayload::postcard(POLICY_ACTION_SECTION, POLICY_ACTIONS_SCHEMA, &bindings)?;
    if section.payload.len() > MAX_ACTION_SECTION_BYTES {
        return Err(ContainerError::message(
            "ASTRA_VN_POLICY_ACTION_PACKAGE_LIMIT",
        ));
    }
    Ok(section)
}

pub fn load_policy_actions(
    package: &PackageReader,
) -> Result<Vec<PolicyActionBinding>, ContainerError> {
    let Some(section) = package.container().section_entry(POLICY_ACTION_SECTION) else {
        return Ok(Vec::new());
    };
    if section.schema != POLICY_ACTIONS_SCHEMA {
        return Err(ContainerError::message(
            "ASTRA_VN_POLICY_ACTION_PACKAGE_VERSION",
        ));
    }
    let bytes = package
        .container()
        .read_bounded(POLICY_ACTION_SECTION, MAX_ACTION_SECTION_BYTES)?;
    let bindings: Vec<PolicyActionBinding> = postcard::from_bytes(&bytes)
        .map_err(|_| ContainerError::message("ASTRA_VN_POLICY_ACTION_PACKAGE_INVALID"))?;
    validate(&bindings)?;
    Ok(bindings)
}

fn validate(bindings: &[PolicyActionBinding]) -> Result<(), ContainerError> {
    if bindings.len() > 64 {
        return Err(ContainerError::message(
            "ASTRA_VN_POLICY_ACTION_PACKAGE_LIMIT",
        ));
    }
    let mut commands = BTreeSet::new();
    let mut entries = BTreeMap::new();
    for binding in bindings {
        binding
            .validate()
            .map_err(|_| ContainerError::message("ASTRA_VN_POLICY_ACTION_PACKAGE_INVALID"))?;
        if binding.read_text.iter().any(|uri| uri.prefix() != "asset") {
            return Err(ContainerError::message("ASTRA_VN_POLICY_ACTION_ASSET_ID"));
        }
        if entries
            .insert(&binding.entry, (&binding.source_hash, &binding.read_text))
            .is_some_and(|prior| prior != (&binding.source_hash, &binding.read_text))
        {
            return Err(ContainerError::message(
                "ASTRA_VN_POLICY_ACTION_ENTRY_CONFLICT",
            ));
        }
        if !commands.insert(&binding.descriptor.command) {
            return Err(ContainerError::message("ASTRA_VN_POLICY_ACTION_DUPLICATE"));
        }
    }
    Ok(())
}
