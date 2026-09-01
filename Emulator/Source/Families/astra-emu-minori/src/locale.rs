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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MinoriLocaleHook;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MinoriLocaleError {
    #[error("ASTRA_EMU_MINORI_LOCALE_HOOK: unsupported locale hook")]
    UnsupportedHook,
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
}
