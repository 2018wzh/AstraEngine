use super::*;
use crate::host_api::RfvpFileInfo;
use crate::script::Variant;
use bincode::Options;

#[derive(Default)]
struct MemoryFs {
    bytes: Option<Vec<u8>>,
    failure: Option<RfvpError>,
}

struct MemoryFile(Vec<u8>);

impl RfvpFile for MemoryFile {
    fn len(&mut self) -> RfvpResult<u64> {
        Ok(self.0.len() as u64)
    }
    fn read_at(&mut self, offset: u64, out: &mut [u8]) -> RfvpResult<usize> {
        let start = usize::try_from(offset).map_err(|_| RfvpError::InvalidArgument)?;
        let source = self.0.get(start..).ok_or(RfvpError::EndOfFile)?;
        let count = out.len().min(source.len());
        out[..count].copy_from_slice(&source[..count]);
        Ok(count)
    }
}

impl RfvpFileSystem for MemoryFs {
    type File = MemoryFile;
    fn open(&mut self, path: &str) -> RfvpResult<Self::File> {
        assert_eq!(path, PATH);
        if let Some(error) = self.failure {
            return Err(error);
        }
        self.bytes
            .clone()
            .map(MemoryFile)
            .ok_or(RfvpError::NotFound)
    }
    fn metadata(&mut self, _: &str) -> RfvpResult<RfvpFileInfo> {
        unreachable!("loading must propagate open errors directly")
    }
    fn write_all(&mut self, path: &str, bytes: &[u8]) -> RfvpResult<()> {
        assert_eq!(path, PATH);
        if let Some(error) = self.failure {
            return Err(error);
        }
        self.bytes = Some(bytes.to_vec());
        Ok(())
    }
}

fn game() -> GameData {
    let mut game = GameData::default();
    game.init_hosted_globals(2, 3);
    game
}

#[test]
fn cold_load_restores_system_region_flags_and_read_state_without_rolling_back_slots() {
    let mut original = game();
    original.globals.set(0, Variant::Int(77));
    original.globals.set(2, Variant::Int(42));
    original.globals.set(4, Variant::True);
    original.flag_manager.set_flag(3, 5, true);
    original.motion_manager.text_manager.readed_text[27] = 0x1234;
    original.save_manager.set_thumb_size(32, 24);
    let mut fs = MemoryFs::default();
    save(&original, &mut fs).unwrap();
    drop(original);

    let mut cold = game();
    cold.globals.set(0, Variant::Int(99));
    assert!(load(&mut cold, &mut fs).unwrap());
    assert!(matches!(cold.globals.get(0), Some(Variant::Int(99))));
    assert!(matches!(cold.globals.get(2), Some(Variant::Int(42))));
    assert!(matches!(cold.globals.get(4), Some(Variant::True)));
    assert!(cold.flag_manager.get_flag(3, 5));
    assert_eq!(cold.motion_manager.text_manager.readed_text[27], 0x1234);
    assert_eq!(cold.save_manager.get_thumb_width(), 32);
    assert_eq!(cold.save_manager.get_thumb_height(), 24);
    assert!(matches!(game().globals.get(2), Some(Variant::Nil)));

    // The hosted writer uses the native RFVP bincode options and RFVG footer,
    // rather than introducing a second codec or host-specific envelope.
    let bytes = fs.bytes.as_ref().unwrap();
    let footer = bytes.len() - 8;
    assert_eq!(&bytes[footer + 4..], b"RFVG");
    let decoded: GlobalSaveDataV1 = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
        .deserialize(&bytes[..footer])
        .unwrap();
    assert!(matches!(decoded.volatile_globals[0], Variant::Int(42)));
}

#[test]
fn missing_save_is_optional_but_io_errors_and_failed_writes_are_not() {
    let mut game = game();
    let mut fs = MemoryFs::default();
    assert!(!load(&mut game, &mut fs).unwrap());
    save(&game, &mut fs).unwrap();
    let original = fs.bytes.clone();
    fs.failure = Some(RfvpError::Io);
    assert_eq!(load(&mut game, &mut fs), Err(RfvpError::Io));
    assert_eq!(save(&game, &mut fs), Err(RfvpError::Io));
    assert_eq!(fs.bytes, original);
}

#[test]
fn invalid_global_save_is_rejected_before_any_state_changes() {
    let mut game = game();
    game.globals.set(2, Variant::Int(42));
    let baseline = GlobalSaveDataV1::capture(&game);
    for kind in 0..5 {
        let mut invalid = baseline.clone();
        invalid.flags.set_flag(3, 5, true);
        match kind {
            0 => invalid.version = 2,
            1 => invalid.non_volatile_global_count += 1,
            2 => invalid.volatile_global_count += 1,
            3 => {
                invalid.volatile_globals.pop();
            }
            _ => {
                invalid.readed_text.pop();
            }
        }
        let mut fs = MemoryFs {
            bytes: Some(encode_global_savedata_v1(&invalid).unwrap()),
            failure: None,
        };
        assert_eq!(load(&mut game, &mut fs), Err(RfvpError::InvalidData));
        assert!(!game.flag_manager.get_flag(3, 5));
        assert!(matches!(game.globals.get(2), Some(Variant::Int(42))));
    }
    for bytes in [
        Vec::new(),
        b"invalid".to_vec(),
        b"\x00\x01\x00\x00RFVG".to_vec(),
    ] {
        let mut fs = MemoryFs {
            bytes: Some(bytes),
            failure: None,
        };
        assert_eq!(load(&mut game, &mut fs), Err(RfvpError::InvalidData));
    }
}
