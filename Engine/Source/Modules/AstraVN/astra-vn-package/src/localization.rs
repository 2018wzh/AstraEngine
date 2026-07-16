use std::{collections::BTreeMap, fmt};

use astra_package::{ContainerError, PackageReader};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const VN_LOCALIZATION_TABLE_SCHEMA: &str = "astra.vn.localization_table.v1";
pub const PLAYER_LOCALE_CONFIG_SCHEMA: &str = "astra.player_locale_config.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VnLocalizationTable {
    pub schema: String,
    pub locale: String,
    #[serde(deserialize_with = "deserialize_unique_strings")]
    pub strings: BTreeMap<String, String>,
}

impl VnLocalizationTable {
    pub fn validate(&self, expected_locale: &str) -> Result<(), ContainerError> {
        validate_locale_id(expected_locale)?;
        if self.schema != VN_LOCALIZATION_TABLE_SCHEMA
            || self.locale != expected_locale
            || self.strings.is_empty()
            || self.strings.len() > 65_536
            || self.strings.iter().any(|(key, value)| {
                key.trim().is_empty() || key.len() > 256 || value.len() > 1024 * 1024
            })
        {
            return Err(ContainerError::message(
                "ASTRA_VN_LOCALIZATION_IDENTITY: localization table identity or bounds are invalid",
            ));
        }
        Ok(())
    }

    pub fn resolve(&self, key: &str) -> Result<&str, ContainerError> {
        self.strings.get(key).map(String::as_str).ok_or_else(|| {
            ContainerError::message(format!(
                "ASTRA_VN_LOCALIZATION_KEY_MISSING: locale {} has no key {key}",
                self.locale
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayerLocaleConfig {
    pub schema: String,
    pub default_locale: String,
    pub available_locales: Vec<String>,
}

impl PlayerLocaleConfig {
    pub fn validate(&self) -> Result<(), ContainerError> {
        validate_locale_id(&self.default_locale)?;
        let mut previous = None;
        for locale in &self.available_locales {
            validate_locale_id(locale)?;
            if previous.is_some_and(|previous: &str| previous >= locale.as_str()) {
                return Err(ContainerError::message(
                    "ASTRA_PLAYER_LOCALE_ORDER: available locales must be sorted and unique",
                ));
            }
            previous = Some(locale.as_str());
        }
        if self.schema != PLAYER_LOCALE_CONFIG_SCHEMA
            || self.available_locales.is_empty()
            || self.available_locales.len() > 64
            || self
                .available_locales
                .binary_search(&self.default_locale)
                .is_err()
        {
            return Err(ContainerError::message(
                "ASTRA_PLAYER_LOCALE_CONFIG: locale config identity is invalid",
            ));
        }
        Ok(())
    }
}

pub fn load_localization(
    package: &PackageReader,
    locale: &str,
    max_bytes: usize,
) -> Result<VnLocalizationTable, ContainerError> {
    validate_locale_id(locale)?;
    let section_id = format!("vn.localization.{locale}");
    let entry = package
        .container()
        .section_entry(&section_id)
        .ok_or_else(|| {
            ContainerError::message(format!(
                "ASTRA_VN_LOCALIZATION_MISSING: package has no section {section_id}"
            ))
        })?;
    if entry.schema != VN_LOCALIZATION_TABLE_SCHEMA {
        return Err(ContainerError::message(
            "ASTRA_VN_LOCALIZATION_SCHEMA: localization section schema is unsupported",
        ));
    }
    let bytes = package.container().read_bounded(&section_id, max_bytes)?;
    let table: VnLocalizationTable = serde_json::from_slice(&bytes).map_err(|error| {
        ContainerError::message(format!("ASTRA_VN_LOCALIZATION_DECODE: {error}"))
    })?;
    table.validate(locale)?;
    Ok(table)
}

pub fn load_player_locale_config(
    package: &PackageReader,
) -> Result<PlayerLocaleConfig, ContainerError> {
    let entry = package
        .container()
        .section_entry("player.locale_config")
        .ok_or_else(|| {
            ContainerError::message(
                "ASTRA_PLAYER_LOCALE_CONFIG_MISSING: package has no locale config",
            )
        })?;
    if entry.schema != PLAYER_LOCALE_CONFIG_SCHEMA {
        return Err(ContainerError::message(
            "ASTRA_PLAYER_LOCALE_CONFIG_SCHEMA: locale config schema is unsupported",
        ));
    }
    let bytes = package
        .container()
        .read_bounded("player.locale_config", 64 * 1024)?;
    let config: PlayerLocaleConfig = serde_json::from_slice(&bytes).map_err(|error| {
        ContainerError::message(format!("ASTRA_PLAYER_LOCALE_CONFIG_DECODE: {error}"))
    })?;
    config.validate()?;
    for locale in &config.available_locales {
        load_localization(package, locale, 16 * 1024 * 1024)?;
    }
    Ok(config)
}

pub fn validate_locale_id(locale: &str) -> Result<(), ContainerError> {
    if locale.is_empty()
        || locale.len() > 64
        || !locale
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ContainerError::message(
            "ASTRA_PLAYER_LOCALE_IDENTITY: locale id is unsafe",
        ));
    }
    Ok(())
}

fn deserialize_unique_strings<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct UniqueStringsVisitor;

    impl<'de> serde::de::Visitor<'de> for UniqueStringsVisitor {
        type Value = BTreeMap<String, String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a localization key/value object without duplicate keys")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut strings = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if strings.insert(key.clone(), value).is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate localization key {key}"
                    )));
                }
            }
            Ok(strings)
        }
    }

    deserializer.deserialize_map(UniqueStringsVisitor)
}
