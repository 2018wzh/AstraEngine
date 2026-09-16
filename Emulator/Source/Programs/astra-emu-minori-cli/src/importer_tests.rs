use super::*;
use std::io::Write;

struct Fixture {
    bytes: Vec<u8>,
    next_id: i32,
}

impl Fixture {
    fn new() -> Self {
        let mut bytes = vec![0];
        for value in [1_i32, -1, 1, 0] {
            bytes.extend(value.to_le_bytes());
        }
        Self { bytes, next_id: 1 }
    }
    fn id(&mut self) {
        self.bytes.extend(self.next_id.to_le_bytes());
        self.next_id += 1;
    }
    fn text(&mut self, text: &str) {
        assert!(text.len() < 128);
        self.bytes.push(text.len() as u8);
        self.bytes.extend(text.as_bytes());
    }
    fn object(&mut self, class: &str, members: &[&str]) {
        self.bytes.push(4); // SystemClassWithMembersAndTypes
        self.id();
        self.text(class);
        self.bytes.extend((members.len() as i32).to_le_bytes());
        for name in members {
            self.text(name);
        }
        self.bytes.extend(std::iter::repeat_n(2, members.len())); // Object members
    }
    fn string(&mut self, text: &str) {
        self.bytes.push(6);
        self.id();
        self.text(text);
    }
    fn key(&mut self) {
        self.bytes.push(15);
        self.id();
        self.bytes.extend(4_i32.to_le_bytes());
        self.bytes.extend([2, 1, 2, 3, 4]); // Primitive Byte plus public minimal key
    }

    fn write(mut self, path: &Path) {
        self.bytes.push(11);
        let mut compressed =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressed.write_all(&self.bytes).unwrap();
        let mut formats = b"GARbroDB\0\0\0\0".to_vec();
        formats.extend(compressed.finish().unwrap());
        std::fs::write(path, formats).unwrap();
    }
}

#[test]
fn catalog_excludes_unrelated_values_and_rejects_duplicate_titles() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Formats.dat");
    for duplicate in [false, true] {
        let mut fixture = Fixture::new();
        fixture.object("Fixture.Root", &["first", "second"]);
        for index in 0..2 {
            fixture.object("Fixture.Pair", &["key", "value"]);
            fixture.string(if index == 0 || duplicate {
                "Public Fixture"
            } else {
                "private-field"
            });
            fixture.object(
                if index == 0 || duplicate {
                    "GameRes.Formats.Musica.PazScheme"
                } else {
                    "Fixture.PazScheme"
                },
                &["Secret"],
            );
            fixture.string("private-value");
        }
        fixture.write(&path);
        if duplicate {
            assert_eq!(
                list_titles(&path).unwrap_err().to_string(),
                "ASTRA_EMU_GARBRO_TITLE_DUPLICATE"
            );
        } else {
            assert_eq!(list_titles(&path).unwrap(), ["Public Fixture"]);
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn imports_valid_nrbf_into_one_typed_json_profile_without_executable_patch() {
    let mut fixture = Fixture::new();
    fixture.object("Fixture.Pair", &["key", "value"]);
    fixture.string("Public Fixture");
    fixture.object("GameRes.Formats.Musica.PazScheme", &["Version", "ArcKeys"]);
    fixture.bytes.extend([8, 8]); // MemberPrimitiveTyped(Int32)
    fixture.bytes.extend(0_i32.to_le_bytes());
    fixture.bytes.push(16);
    fixture.id();
    fixture
        .bytes
        .extend((REQUIRED_ARCHIVE_ROLES.len() as i32).to_le_bytes());
    for role in REQUIRED_ARCHIVE_ROLES {
        fixture.object("Fixture.Pair", &["key", "value"]);
        fixture.string(role);
        fixture.object("Fixture.PazKey", &["IndexKey", "DataKey"]);
        fixture.key();
        fixture.key();
    }
    fixture.bytes.push(11);
    let mut compressed =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    compressed.write_all(&fixture.bytes).unwrap();
    let mut formats = b"GARbroDB\0\0\0\0".to_vec();
    formats.extend(compressed.finish().unwrap());
    let directory = tempfile::tempdir().unwrap();
    let formats_path = directory.path().join("Formats.dat");
    std::fs::write(&formats_path, formats).unwrap();
    assert_eq!(list_titles(&formats_path).unwrap(), ["Public Fixture"]);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    import(&formats_path, "Public Fixture", directory.path()).unwrap();
    let output = directory.path().join(PROFILE_NAME);
    let profile: MinoriProfile = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(profile.schema, MINORI_PROFILE_SCHEMA);
    assert_eq!(profile.paz_version, 0);
    assert_eq!(profile.roles.len(), REQUIRED_ARCHIVE_ROLES.len());
    assert_eq!(profile.roles["bg"].index_key, [1, 2, 3, 4]);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
}
