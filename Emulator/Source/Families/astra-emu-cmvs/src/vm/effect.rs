//! Recovered CMVS effect-channel object model.
//!
//! The original engine stores one 0x3088-byte channel object per effect
//! channel. Its quad records live at a 76-byte (38-word) stride, and the
//! recovered handlers address it with `_WORD *` arithmetic. Modelling the
//! object as a sparse word-addressed image keeps every recovered read and
//! write at the original byte/word offset instead of inventing a new layout.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The recovered text surface attached to one effect playback record.
///
/// `sub_462C10` (case 286) stores one value dword and two script strings, then
/// marks the surface dirty. The metric-producing cases (`sub_463CC0`/
/// `sub_463750`) read those strings through the script. The stored state keeps
/// only the numeric reference, length and flags so game text never enters the
/// snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsEffectTextSurface {
    /// Dword 320: the surface's stored value.
    pub value: u32,
    /// The first stored script string reference (`record + 324`).
    pub first: Option<crate::CmvsPs2aPrivateStringReference>,
    /// The second stored script string reference (`record + 388`).
    pub second: Option<crate::CmvsPs2aPrivateStringReference>,
    /// Byte length of the first stored string.
    pub first_length: u32,
    /// Byte length of the second stored string.
    pub second_length: u32,
}

/// One entry of the recovered effect playback table (`game[750]`, twelve
/// entries registered by case 349). Cases 276/277/278 write the fields the
/// original stores at record dwords 41/44/45.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsEffectPlaybackRecord {
    /// Record dword 41 (`sub_464AA0`, case 276).
    pub flag: u32,
    /// Record dword 44 (`sub_464A90`, case 277).
    pub value: u32,
    /// Record dword 45 (`sub_464A80`, case 278).
    pub value2: u32,
    /// Case 279 (`sub_488EF0`) forwards a boolean to `sub_429850`, a
    /// graphics-subsystem global the recovered subset keeps as state here.
    pub global_flag: u32,
    /// The record's text surface (`sub_462C10`, case 286).
    pub text_surface: CmvsEffectTextSurface,
}

impl CmvsEffectPlaybackRecord {
    /// Writes the recovered record field for cases 276/277/278/279.
    pub fn set_field(&mut self, field_offset: u32, value: u32) {
        match field_offset {
            41 => self.flag = value,
            44 => self.value = value,
            45 => self.value2 = value,
            _ => self.global_flag = value,
        }
    }
}

/// One child element of an effect channel's element container
/// (`channel[630]`, dereferenced through `sub_444EF0(container, id)`).
///
/// Cases 378/379/380 address one element by `(channel, child)`:
/// `sub_445EA0` writes the visibility dword, `sub_466AE0` answers the
/// existence query, and `sub_445E80` stores the element size.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsEffectElement {
    /// Element dword 1035 (`sub_445EA0`, cases 402/403 and 378).
    pub visible: u32,
    /// Element width stored by `sub_445E80` (case 380).
    pub width: i32,
    /// Element height stored by `sub_445E80` (case 380).
    pub height: i32,
}

/// One effect channel object: a sparse word-addressed image of the original
/// 0x3088-byte channel, holding 32 quad records at a 38-word stride.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsEffectChannel {
    pub words: BTreeMap<u32, u16>,
}

impl CmvsEffectChannel {
    /// Words per quad record: the original uses `this + 38 * quad` on a
    /// `_WORD *` (76 bytes per record).
    pub const QUAD_STRIDE_WORDS: u32 = 38;

    /// Reads the little-endian dword at a word index, like the original's
    /// `*(_DWORD *)(word_base + n)`.
    pub fn read_dword(&self, word: u32) -> u32 {
        let low = u32::from(self.words.get(&word).copied().unwrap_or(0));
        let high = u32::from(self.words.get(&(word + 1)).copied().unwrap_or(0));
        low | (high << 16)
    }

    /// Writes the little-endian dword at a word index.
    pub fn write_dword(&mut self, word: u32, value: u32) {
        self.words.insert(word, value as u16);
        self.words.insert(word + 1, (value >> 16) as u16);
    }

    pub fn read_i16(&self, word: u32) -> i16 {
        self.words.get(&word).copied().unwrap_or(0) as i16
    }

    pub fn write_i16(&mut self, word: u32, value: i16) {
        self.words.insert(word, value as u16);
    }

    /// `sub_47F270` (case 333) reads the channel object's dword at word 4.
    pub fn activity(&self) -> bool {
        self.read_dword(4) != 0
    }

    /// The channel origin stored at words 3/4 (byte 12/16); `sub_466870`'s
    /// hit test adds it to every quad rectangle.
    pub fn origin(&self) -> (i32, i32) {
        (self.read_dword(3) as i32, self.read_dword(4) as i32)
    }

    /// `sub_4678C0` (case 336) writes the origin to words 3/4 and mirrors it
    /// into the recovered duplicate pair at words 620/621.
    pub fn set_origin(&mut self, x: i32, y: i32) {
        self.write_dword(3, x as u32);
        self.write_dword(4, y as u32);
        self.write_dword(620, x as u32);
        self.write_dword(621, y as u32);
    }

    /// `sub_4679B0` (case 337) stores the channel's playback mode at word 28.
    /// Modes 0/2 clear the dword at word 30; modes 1/3 store the two given
    /// `__int16` coordinates at words 30/32.
    pub fn set_playback_mode(&mut self, mode: u16, first: i16, second: i16) {
        self.words.insert(28, mode);
        match mode {
            0 | 2 => {
                self.write_dword(30, 0);
            }
            1 | 3 => {
                self.write_i16(30, first);
                self.write_i16(32, second);
            }
            _ => {}
        }
    }

    /// The channel's stored playback mode (word 28) and coordinates.
    pub fn playback_mode(&self) -> u16 {
        self.words.get(&28).copied().unwrap_or(0)
    }

    /// `sub_467CF0` (case 346) stores two dwords at 618/619.
    pub fn set_value_pair(&mut self, first: u32, second: u32) {
        self.write_dword(618, first);
        self.write_dword(619, second);
    }

    /// `sub_467CE0` (case 347) stores the flag dword at 622.
    pub fn set_flag(&mut self, value: bool) {
        self.write_dword(622, u32::from(value));
    }

    /// `sub_466D50`/`sub_466D90`/`sub_47F470` (cases 402/403/405) address the
    /// quad record's visibility dword at byte `76*quad + 40`, i.e. word
    /// `38*quad + 20`.
    fn quad_visibility_word(quad: u32) -> u32 {
        Self::QUAD_STRIDE_WORDS * quad + 20
    }

    pub fn quad_visible(&self, quad: u32) -> bool {
        self.read_dword(Self::quad_visibility_word(quad)) != 0
    }

    pub fn set_quad_visible(&mut self, quad: u32, visible: bool) {
        self.write_dword(Self::quad_visibility_word(quad), u32::from(visible));
    }

    /// `sub_466CB0` (case 400) writes six `__int16` geometry words at
    /// `38*quad + 6*sub + 22`.
    pub fn write_quad_geometry(&mut self, quad: u32, sub: u32, values: [i16; 6]) {
        let base = Self::QUAD_STRIDE_WORDS * quad + 6 * sub + 22;
        for (index, value) in values.iter().enumerate() {
            self.write_i16(base + index as u32, *value);
        }
    }

    /// `sub_466D10` (case 401) writes the quad's four `__int16` hit-rectangle
    /// words at `38*quad + 52`.
    pub fn write_quad_rect(&mut self, quad: u32, rect: [i16; 4]) {
        let base = Self::QUAD_STRIDE_WORDS * quad + 52;
        for (index, value) in rect.iter().enumerate() {
            self.write_i16(base + index as u32, *value);
        }
    }

    /// The quad's hit rectangle in the channel's local coordinates.
    pub fn quad_rect(&self, quad: u32) -> [i32; 4] {
        let base = Self::QUAD_STRIDE_WORDS * quad + 52;
        [
            i32::from(self.read_i16(base)),
            i32::from(self.read_i16(base + 1)),
            i32::from(self.read_i16(base + 2)),
            i32::from(self.read_i16(base + 3)),
        ]
    }

    /// `sub_47F330` (case 406) stores the selected animation frame at word
    /// `38*quad + 38` (byte `76*quad + 76`) and reads that frame's twelve-byte
    /// geometry sub-record at `18 + 38*quad + 6*frame`.
    pub fn quad_frame(&self, quad: u32) -> u16 {
        self.words
            .get(&(Self::QUAD_STRIDE_WORDS * quad + 38))
            .copied()
            .unwrap_or(0)
    }

    pub fn set_quad_frame(&mut self, quad: u32, frame: u16) {
        self.words
            .insert(Self::QUAD_STRIDE_WORDS * quad + 38, frame);
    }

    /// The four `__int16` rectangle words of one animation frame's geometry
    /// sub-record (`sub_47F330` reads words 4..7 of the 12-byte sub-record).
    pub fn quad_frame_geometry(&self, quad: u32, frame: u16) -> [i16; 4] {
        let base = 18 + Self::QUAD_STRIDE_WORDS * quad + 6 * u32::from(frame);
        [
            self.read_i16(base + 4),
            self.read_i16(base + 5),
            self.read_i16(base + 6),
            self.read_i16(base + 7),
        ]
    }
}
