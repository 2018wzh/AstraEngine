use encoding_rs::{GBK, SHIFT_JIS};
use thiserror::Error;

/// The only text locale accepted by the original Natsuzora installation.
///
/// This is deliberately a locale/code-page binding, not a translation
/// provider.  The host may use the binding when it creates a Japanese locale
/// process, while the family still performs strict CP932 conversion at the
/// byte boundary.
pub const MUSICA_ORIGINAL_VARIANT_ID: &str = "natsuzora-no-perseus.original-ja";
pub const MUSICA_LOCALE_HOOK_ID: &str = "astra.emu.musica.locale.ja-jp.cp932.v1";
/// Simplified-Chinese GBK hook id backed by the eden* official Chinese data.
pub const MUSICA_LOCALE_HOOK_GBK_ID: &str = "astra.emu.musica.locale.zh-hans.gbk.v1";
/// Profile option used by the Musica launch profile to select the source text
/// encoding.  The value names intentionally match the FVP profile contract so
/// the manager can expose one consistent selector across legacy families.
pub const MUSICA_NLS_OPTION: &str = "musica.nls";
pub const MUSICA_NLS_SHIFT_JIS: &str = "shift_jis";
pub const MUSICA_NLS_GBK: &str = "gbk";
pub const MUSICA_NLS_UTF8: &str = "utf8";
/// BCP-47 locale carried by the runtime-open boundary for the original game.
///
/// The hook remains the authority for byte conversion; this value is the
/// presentation/runtime identity used by host composition roots so Japanese
/// text is not opened as an unspecified locale and subsequently rendered with
/// a host-default code-page policy.
pub const MUSICA_RUNTIME_LOCALE: &str = "ja-JP";

/// Encoding values that may be persisted in a Musica launch profile.
///
/// Shift JIS covers the verified Japanese sources; GBK covers the eden*
/// official Chinese data and GBK-encoded patch scripts.  UTF-8 stays
/// reserved: mounting a profile with a reserved value is a hard error and
/// never falls back to another encoding or replacement decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicaNls {
    ShiftJis,
    Gbk,
    Utf8,
}

impl MusicaNls {
    pub fn parse(value: &str) -> Result<Self, MusicaLocaleError> {
        match value {
            MUSICA_NLS_SHIFT_JIS => Ok(Self::ShiftJis),
            MUSICA_NLS_GBK => Ok(Self::Gbk),
            MUSICA_NLS_UTF8 => Ok(Self::Utf8),
            _ => Err(MusicaLocaleError::UnsupportedEncoding),
        }
    }

    pub const fn profile_value(self) -> &'static str {
        match self {
            Self::ShiftJis => MUSICA_NLS_SHIFT_JIS,
            Self::Gbk => MUSICA_NLS_GBK,
            Self::Utf8 => MUSICA_NLS_UTF8,
        }
    }

    pub const fn is_currently_supported(self) -> bool {
        matches!(self, Self::ShiftJis | Self::Gbk)
    }

    /// The locale hook id that must accompany this NLS selection.
    pub const fn locale_hook_id(self) -> &'static str {
        match self {
            Self::ShiftJis => MUSICA_LOCALE_HOOK_ID,
            Self::Gbk => MUSICA_LOCALE_HOOK_GBK_ID,
            Self::Utf8 => "",
        }
    }
}

/// Locale/code-page hook selected by the launch profile NLS option.
///
/// The hook is the only authority for byte conversion at the family boundary;
/// it never falls back to replacement decoding or to the system code page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicaLocaleHook {
    JapaneseCp932,
    SimplifiedChineseGbk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MusicaLocaleError {
    #[error("ASTRA_EMU_MUSICA_LOCALE_HOOK: unsupported locale hook")]
    UnsupportedHook,
    #[error("ASTRA_EMU_MUSICA_NLS: unsupported text encoding")]
    UnsupportedEncoding,
    #[error("ASTRA_EMU_MUSICA_LOCALE_DECODE: bytes are not valid in the selected encoding")]
    Decode,
    #[error("ASTRA_EMU_MUSICA_LOCALE_ENCODE: text cannot be encoded in the selected encoding")]
    Encode,
}

impl MusicaLocaleHook {
    pub const fn japanese_cp932() -> Self {
        Self::JapaneseCp932
    }

    pub const fn simplified_chinese_gbk() -> Self {
        Self::SimplifiedChineseGbk
    }

    pub fn from_id(id: &str) -> Result<Self, MusicaLocaleError> {
        match id {
            MUSICA_LOCALE_HOOK_ID => Ok(Self::JapaneseCp932),
            MUSICA_LOCALE_HOOK_GBK_ID => Ok(Self::SimplifiedChineseGbk),
            _ => Err(MusicaLocaleError::UnsupportedHook),
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::JapaneseCp932 => MUSICA_LOCALE_HOOK_ID,
            Self::SimplifiedChineseGbk => MUSICA_LOCALE_HOOK_GBK_ID,
        }
    }

    pub fn decode(self, bytes: &[u8]) -> Result<String, MusicaLocaleError> {
        let (decoded, _, had_errors) = match self {
            Self::JapaneseCp932 => SHIFT_JIS.decode(bytes),
            Self::SimplifiedChineseGbk => GBK.decode(bytes),
        };
        if had_errors {
            return Err(MusicaLocaleError::Decode);
        }
        Ok(decoded.into_owned())
    }

    pub fn encode(self, text: &str) -> Result<Vec<u8>, MusicaLocaleError> {
        let (encoded, _, had_errors) = match self {
            Self::JapaneseCp932 => SHIFT_JIS.encode(text),
            Self::SimplifiedChineseGbk => GBK.encode(text),
        };
        if had_errors {
            return Err(MusicaLocaleError::Encode);
        }
        Ok(encoded.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_hook_round_trips_original_title_text() {
        let hook = MusicaLocaleHook::from_id(MUSICA_LOCALE_HOOK_ID).unwrap();
        let encoded = hook.encode("夏空のペルセウス").unwrap();
        assert_eq!(hook.decode(&encoded).unwrap(), "夏空のペルセウス");
    }

    #[test]
    fn japanese_hook_round_trips_original_font_names() {
        let hook = MusicaLocaleHook::japanese_cp932();
        for name in ["ＭＳ Ｐゴシック", "メイリオ", "游ゴシック"] {
            let encoded = hook.encode(name).unwrap();
            assert_eq!(hook.decode(&encoded).unwrap(), name);
        }
    }

    #[test]
    fn locale_hook_rejects_unknown_id_and_malformed_cp932() {
        assert_eq!(
            MusicaLocaleHook::from_id("astra.emu.musica.locale.gbk.v1").unwrap_err(),
            MusicaLocaleError::UnsupportedHook
        );
        assert_eq!(
            MusicaLocaleHook::japanese_cp932()
                .decode(&[0x82])
                .unwrap_err(),
            MusicaLocaleError::Decode
        );
        assert_eq!(
            MusicaLocaleHook::japanese_cp932().encode("🙂").unwrap_err(),
            MusicaLocaleError::Encode
        );
    }

    #[test]
    fn nls_profile_values_match_fvp_and_shift_jis_gbk_are_live() {
        for (value, expected, supported) in [
            (MUSICA_NLS_SHIFT_JIS, MusicaNls::ShiftJis, true),
            (MUSICA_NLS_GBK, MusicaNls::Gbk, true),
            (MUSICA_NLS_UTF8, MusicaNls::Utf8, false),
        ] {
            let parsed = MusicaNls::parse(value).unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.profile_value(), value);
            assert_eq!(parsed.is_currently_supported(), supported);
        }
        assert_eq!(
            MusicaNls::parse("cp936").unwrap_err(),
            MusicaLocaleError::UnsupportedEncoding
        );
    }
}
