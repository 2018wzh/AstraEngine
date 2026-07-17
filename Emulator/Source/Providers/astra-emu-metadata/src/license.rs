use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::MetadataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseUse {
    Development,
    NonCommercial,
    Commercial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MetadataLicenseManifest {
    pub release_use: ReleaseUse,
    pub vndb_commercial_license_id: Option<String>,
}

impl MetadataLicenseManifest {
    pub fn permit_vndb(&self) -> Result<(), MetadataError> {
        if self.release_use != ReleaseUse::Commercial {
            return Ok(());
        }
        let Some(license_id) = self.vndb_commercial_license_id.as_deref() else {
            return Err(MetadataError::LicenseBlocked("vndb-commercial-license"));
        };
        if license_id.is_empty()
            || license_id.len() > 128
            || !license_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(MetadataError::LicenseBlocked("vndb-license-id"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commercial_vndb_requires_explicit_license_manifest() {
        let manifest = MetadataLicenseManifest {
            release_use: ReleaseUse::Commercial,
            vndb_commercial_license_id: None,
        };
        assert!(matches!(
            manifest.permit_vndb(),
            Err(MetadataError::LicenseBlocked(_))
        ));
    }
}
