use encoding_rs::SHIFT_JIS;
use thiserror::Error;

/// The only text locale accepted by the original Natsuzora installation.
///
/// This is deliberately a locale/code-page binding, not a translation
/// provider.  The host may use the binding when it creates a Japanese locale
/// process, while the family still performs strict CP932 conversion at the
/// byte boundary.
pub const MINORI_ORIGINAL_VARIANT_ID: &str = "natsuzora-no-perseus.original-ja";
pub const MINORI_LOCALE_HOOK_ID: &str = "astra.emu.minori.locale.ja-jp.cp932.v1";
/// Profile option used by the Minori launch profile to select the source text
/// encoding.  The value names intentionally match the FVP profile contract so
/// the manager can expose one consistent selector across legacy families.
pub const MINORI_NLS_OPTION: &str = "minori.nls";
pub const MINORI_NLS_SHIFT_JIS: &str = "shift_jis";
pub const MINORI_NLS_GBK: &str = "gbk";
pub const MINORI_NLS_UTF8: &str = "utf8";
/// BCP-47 locale carried by the runtime-open boundary for the original game.
///
/// The hook remains the authority for byte conversion; this value is the
/// presentation/runtime identity used by host composition roots so Japanese
/// text is not opened as an unspecified locale and subsequently rendered with
/// a host-default code-page policy.
pub const MINORI_RUNTIME_LOCALE: &str = "ja-JP";

/// Encoding values that may be persisted in a Minori launch profile.
///
/// Only Shift JIS is currently implemented for the verified Japanese source.
/// The other values are deliberately represented here so a profile can be
/// selected before the localized source contract is implemented.  Mounting a
/// profile with one of those reserved values is a hard error; it never falls
/// back to CP932 or to replacement decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinoriNls {
    ShiftJis,
    Gbk,
    Utf8,
}

impl MinoriNls {
    pub fn parse(value: &str) -> Result<Self, MinoriLocaleError> {
        match value {
            MINORI_NLS_SHIFT_JIS => Ok(Self::ShiftJis),
            MINORI_NLS_GBK => Ok(Self::Gbk),
            MINORI_NLS_UTF8 => Ok(Self::Utf8),
            _ => Err(MinoriLocaleError::UnsupportedEncoding),
        }
    }

    pub const fn profile_value(self) -> &'static str {
        match self {
            Self::ShiftJis => MINORI_NLS_SHIFT_JIS,
            Self::Gbk => MINORI_NLS_GBK,
            Self::Utf8 => MINORI_NLS_UTF8,
        }
    }

    pub const fn is_currently_supported(self) -> bool {
        matches!(self, Self::ShiftJis)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MinoriLocaleHook;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MinoriLocaleError {
    #[error("ASTRA_EMU_MINORI_LOCALE_HOOK: unsupported locale hook")]
    UnsupportedHook,
    #[error("ASTRA_EMU_MINORI_NLS: unsupported text encoding")]
    UnsupportedEncoding,
    #[error("ASTRA_EMU_MINORI_LOCALE_DECODE: bytes are not valid CP932")]
    Decode,
    #[error("ASTRA_EMU_MINORI_LOCALE_ENCODE: text cannot be encoded as CP932")]
    Encode,
}

impl MinoriLocaleHook {
    pub const fn japanese_cp932() -> Self {
        Self
    }

    pub fn from_id(id: &str) -> Result<Self, MinoriLocaleError> {
        (id == MINORI_LOCALE_HOOK_ID)
            .then_some(Self::japanese_cp932())
            .ok_or(MinoriLocaleError::UnsupportedHook)
    }

    pub const fn id(self) -> &'static str {
        MINORI_LOCALE_HOOK_ID
    }

    pub fn decode(self, bytes: &[u8]) -> Result<String, MinoriLocaleError> {
        SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|value| value.into_owned())
            .ok_or(MinoriLocaleError::Decode)
    }

    pub fn encode(self, text: &str) -> Result<Vec<u8>, MinoriLocaleError> {
        let (encoded, _, malformed) = SHIFT_JIS.encode(text);
        if malformed {
            return Err(MinoriLocaleError::Encode);
        }
        Ok(encoded.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_hook_round_trips_original_title_text() {
        let hook = MinoriLocaleHook::from_id(MINORI_LOCALE_HOOK_ID).unwrap();
        let encoded = hook.encode("夏空のペルセウス").unwrap();
        assert_eq!(hook.decode(&encoded).unwrap(), "夏空のペルセウス");
    }

    #[test]
    fn japanese_hook_round_trips_original_font_names() {
        let hook = MinoriLocaleHook::japanese_cp932();
        for name in ["ＭＳ Ｐゴシック", "メイリオ", "游ゴシック"] {
            let encoded = hook.encode(name).unwrap();
            assert_eq!(hook.decode(&encoded).unwrap(), name);
        }
    }

    #[test]
    fn locale_hook_rejects_unknown_id_and_malformed_cp932() {
        assert_eq!(
            MinoriLocaleHook::from_id("astra.emu.minori.locale.gbk.v1").unwrap_err(),
            MinoriLocaleError::UnsupportedHook
        );
        assert_eq!(
            MinoriLocaleHook::japanese_cp932()
                .decode(&[0x82])
                .unwrap_err(),
            MinoriLocaleError::Decode
        );
        assert_eq!(
            MinoriLocaleHook::japanese_cp932().encode("🙂").unwrap_err(),
            MinoriLocaleError::Encode
        );
    }

    #[test]
    fn nls_profile_values_match_fvp_and_only_shift_jis_is_live() {
        for (value, expected, supported) in [
            (MINORI_NLS_SHIFT_JIS, MinoriNls::ShiftJis, true),
            (MINORI_NLS_GBK, MinoriNls::Gbk, false),
            (MINORI_NLS_UTF8, MinoriNls::Utf8, false),
        ] {
            let parsed = MinoriNls::parse(value).unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.profile_value(), value);
            assert_eq!(parsed.is_currently_supported(), supported);
        }
        assert_eq!(
            MinoriNls::parse("cp936").unwrap_err(),
            MinoriLocaleError::UnsupportedEncoding
        );
    }
}
