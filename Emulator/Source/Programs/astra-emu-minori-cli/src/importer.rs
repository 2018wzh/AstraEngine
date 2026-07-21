use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use astra_emu_family_support::{
    enforce_private_file_permissions, LegacyVfsMountProfile, VFS_MOUNT_PROFILE_SCHEMA,
};
use astra_emu_minori::{
    MinoriCacheOptions, MinoriFamilyOptions, MinoriPrivateProfilePayload, MinoriRolePrivateProfile,
    MINORI_FAMILY_OPTIONS_SCHEMA, MINORI_PRIVATE_PROFILE_SCHEMA, REQUIRED_ARCHIVE_ROLES,
};
use flate2::read::ZlibDecoder;

use crate::garbro_nrbf::{NrbfGraph, NrbfValue};

const PATCH_NAME: &str = "astraemu.patch.luau";
const PROFILE_NAME: &str = "astraemu.minori.mount.yaml";
const MAX_GRAPH_NODES: usize = 1_000_000;
const MAX_GRAPH_DEPTH: usize = 128;
const MAX_DICTIONARY_ENTRIES: usize = 100_000;

#[derive(Debug)]
struct ImportedRole {
    index_key: Vec<u8>,
    data_key: Vec<u8>,
    type_keys: BTreeMap<String, String>,
    version: i32,
}

pub fn import(
    formats: &Path,
    title: &str,
    game_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if title.is_empty() || title.len() > 512 {
        return Err("ASTRA_EMU_GARBRO_TITLE".into());
    }
    if !game_dir.is_dir() {
        return Err("ASTRA_EMU_GARBRO_GAME_DIRECTORY".into());
    }
    let patch_path = game_dir.join(PATCH_NAME);
    let profile_path = game_dir.join(PROFILE_NAME);
    let patch_temp = game_dir.join(format!(".{PATCH_NAME}.tmp"));
    let profile_temp = game_dir.join(format!(".{PROFILE_NAME}.tmp"));
    if [&patch_path, &profile_path, &patch_temp, &profile_temp]
        .into_iter()
        .any(|path| path.exists())
    {
        return Err("ASTRA_EMU_GARBRO_OUTPUT_EXISTS".into());
    }
    let bytes = fs::read(formats)?;
    if bytes.len() < 12 || &bytes[..8] != b"GARbroDB" {
        return Err("ASTRA_EMU_GARBRO_HEADER".into());
    }
    let mut decoded = Vec::new();
    ZlibDecoder::new(&bytes[12..])
        .take(256 * 1024 * 1024)
        .read_to_end(&mut decoded)?;
    if decoded.len() >= 256 * 1024 * 1024 {
        return Err("ASTRA_EMU_GARBRO_SIZE".into());
    }
    let graph = NrbfGraph::parse(&decoded).map_err(|error| error.code())?;
    let root = graph.root().map_err(|_| "ASTRA_EMU_GARBRO_ROOT")?;
    let records = find_dictionary_values(&graph, root, title)?;
    let record = match records.as_slice() {
        [record] => *record,
        [] => return Err("ASTRA_EMU_GARBRO_TITLE_NOT_FOUND".into()),
        _ => return Err("ASTRA_EMU_GARBRO_TITLE_DUPLICATE".into()),
    };
    let roles = extract_roles(&graph, record)?;
    let version = roles
        .values()
        .next()
        .ok_or("ASTRA_EMU_GARBRO_ROLE_MISSING")?
        .version;
    if roles.values().any(|role| role.version != version) {
        return Err("ASTRA_EMU_GARBRO_VERSION_CONFLICT".into());
    }
    let private = MinoriPrivateProfilePayload {
        schema: MINORI_PRIVATE_PROFILE_SCHEMA.into(),
        roles: roles
            .into_iter()
            .map(|(role, value)| {
                (
                    role,
                    MinoriRolePrivateProfile {
                        index_key: value.index_key,
                        data_key: value.data_key,
                        type_passwords: value.type_keys,
                        archive_xor: None,
                        video_key: None,
                    },
                )
            })
            .collect(),
    };
    let private_bytes = serde_json::to_vec(&private)?;
    let patch = format!("astra.family.register_private_profile({{ id = \"minori.paz\", schema = \"{}\", payload = buffer.fromstring(\"{}\") }})\n", MINORI_PRIVATE_PROFILE_SCHEMA, bytes_escape(&private_bytes));
    let options = MinoriFamilyOptions {
        paz_version: u8::try_from(version).map_err(|_| "ASTRA_EMU_GARBRO_VERSION")?,
        index_size_xor: 0,
        private_profile_id: "minori.paz".into(),
        private_profile_schema: MINORI_PRIVATE_PROFILE_SCHEMA.into(),
        cache: MinoriCacheOptions {
            enabled: true,
            total_bytes: 8 * 1024 * 1024 * 1024,
            entry_bytes: 1024 * 1024 * 1024,
        },
        archive_roles: REQUIRED_ARCHIVE_ROLES
            .into_iter()
            .map(str::to_owned)
            .collect(),
    };
    let profile = LegacyVfsMountProfile {
        schema: VFS_MOUNT_PROFILE_SCHEMA.into(),
        profile_id: "minori.local".into(),
        family_id: "minori".into(),
        mount_id: "minori-main".into(),
        prefix: "minori:/".into(),
        private_patch: PATCH_NAME.into(),
        family_options_schema: MINORI_FAMILY_OPTIONS_SCHEMA.into(),
        family_options: serde_json::to_value(options)?,
    };
    let profile_bytes = serde_yaml::to_string(&profile)?.into_bytes();
    write_new_private(&patch_temp, patch.as_bytes())?;
    if let Err(error) = write_new_private(&profile_temp, &profile_bytes) {
        rollback(&[&patch_temp])?;
        return Err(error);
    }
    if let Err(error) = fs::rename(&patch_temp, &patch_path) {
        rollback(&[&patch_temp, &profile_temp])?;
        return Err(error.into());
    }
    if let Err(error) = fs::rename(&profile_temp, &profile_path) {
        rollback(&[&profile_temp, &patch_path])?;
        return Err(error.into());
    }
    println!("{{\"schema\":\"astra.emu.minori.garbro_import.v2\",\"status\":\"passed\"}}");
    Ok(())
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    enforce_private_file_permissions(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn rollback(paths: &[&Path]) -> Result<(), Box<dyn std::error::Error>> {
    for path in paths {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("ASTRA_EMU_GARBRO_ROLLBACK".into()),
        }
    }
    Ok(())
}

fn bytes_escape(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("\\x{byte:02x}")).collect()
}

fn structural(value: &NrbfValue) -> bool {
    matches!(
        value,
        NrbfValue::Array(_) | NrbfValue::Object(_) | NrbfValue::Ref(_)
    )
}

fn find_dictionary_values<'a>(
    graph: &'a NrbfGraph,
    value: &'a NrbfValue,
    key: &str,
) -> Result<Vec<&'a NrbfValue>, Box<dyn std::error::Error>> {
    let mut stack = vec![(value, 0usize)];
    let mut references = BTreeSet::new();
    let mut visited = 0usize;
    let mut matches = Vec::new();
    while let Some((value, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_GRAPH_NODES {
            return Err("ASTRA_EMU_GARBRO_GRAPH_NODE_LIMIT".into());
        }
        if depth > MAX_GRAPH_DEPTH {
            return Err("ASTRA_EMU_GARBRO_GRAPH_DEPTH_LIMIT".into());
        }
        if let NrbfValue::Ref(id) = value {
            if references.insert(*id) {
                stack.push((
                    graph
                        .dereference(value)
                        .map_err(|_| "ASTRA_EMU_GARBRO_REFERENCE")?,
                    depth,
                ));
            }
            continue;
        }
        match value {
            NrbfValue::Object(object) => {
                let pair_key = object
                    .members
                    .get("key")
                    .or_else(|| object.members.get("Key"))
                    .map(|value| graph.dereference(value))
                    .transpose()
                    .map_err(|_| "ASTRA_EMU_GARBRO_REFERENCE")?;
                if matches!(pair_key, Some(NrbfValue::String(value)) if value == key) {
                    matches.push(
                        object
                            .members
                            .get("value")
                            .or_else(|| object.members.get("Value"))
                            .ok_or("ASTRA_EMU_GARBRO_DICTIONARY_PAIR")?,
                    );
                    if matches.len() > 1 {
                        return Ok(matches);
                    }
                }
                stack.extend(
                    object
                        .members
                        .values()
                        .filter(|value| structural(value))
                        .map(|value| (value, depth + 1)),
                );
            }
            NrbfValue::Array(values) => stack.extend(
                values
                    .iter()
                    .filter(|value| structural(value))
                    .map(|value| (value, depth + 1)),
            ),
            _ => {}
        }
    }
    Ok(matches)
}

fn dictionary_entries<'a>(
    graph: &'a NrbfGraph,
    value: &'a NrbfValue,
) -> Result<Vec<(&'a str, &'a NrbfValue)>, Box<dyn std::error::Error>> {
    let mut stack = vec![(value, 0usize)];
    let mut references = BTreeSet::new();
    let mut visited = 0usize;
    let mut output = Vec::new();
    let mut keys = BTreeSet::new();
    while let Some((value, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_GRAPH_NODES {
            return Err("ASTRA_EMU_GARBRO_GRAPH_NODE_LIMIT".into());
        }
        if depth > MAX_GRAPH_DEPTH {
            return Err("ASTRA_EMU_GARBRO_GRAPH_DEPTH_LIMIT".into());
        }
        if let NrbfValue::Ref(id) = value {
            if references.insert(*id) {
                stack.push((
                    graph
                        .dereference(value)
                        .map_err(|_| "ASTRA_EMU_GARBRO_REFERENCE")?,
                    depth,
                ));
            }
            continue;
        }
        match value {
            NrbfValue::Object(object) => {
                let key = object
                    .members
                    .get("key")
                    .or_else(|| object.members.get("Key"))
                    .map(|value| graph.dereference(value))
                    .transpose()
                    .map_err(|_| "ASTRA_EMU_GARBRO_REFERENCE")?;
                let item = object
                    .members
                    .get("value")
                    .or_else(|| object.members.get("Value"));
                if let (Some(NrbfValue::String(key)), Some(item)) = (key, item) {
                    if !keys.insert(key.to_lowercase()) || output.len() == MAX_DICTIONARY_ENTRIES {
                        return Err("ASTRA_EMU_GARBRO_DICTIONARY_DUPLICATE".into());
                    }
                    output.push((key.as_str(), item));
                }
                stack.extend(
                    object
                        .members
                        .values()
                        .filter(|value| structural(value))
                        .map(|value| (value, depth + 1)),
                );
            }
            NrbfValue::Array(values) => stack.extend(
                values
                    .iter()
                    .filter(|value| structural(value))
                    .map(|value| (value, depth + 1)),
            ),
            _ => {}
        }
    }
    Ok(output)
}

fn object_member<'a>(
    graph: &'a NrbfGraph,
    value: &'a NrbfValue,
    names: &[&str],
) -> Option<&'a NrbfValue> {
    let NrbfValue::Object(object) = graph.dereference(value).ok()? else {
        return None;
    };
    names
        .iter()
        .find_map(|name| object.members.get(*name))
        .and_then(|value| graph.dereference(value).ok())
}

fn extract_roles(
    graph: &NrbfGraph,
    record: &NrbfValue,
) -> Result<BTreeMap<String, ImportedRole>, Box<dyn std::error::Error>> {
    let record = graph
        .dereference(record)
        .map_err(|_| "ASTRA_EMU_GARBRO_REFERENCE")?;
    let class = match record {
        NrbfValue::Object(object) => object.class.as_str(),
        _ => return Err("ASTRA_EMU_GARBRO_OBJECT".into()),
    };
    if !class.ends_with("PazScheme") {
        return Err("ASTRA_EMU_GARBRO_SCHEME_TYPE".into());
    }
    let version = match object_member(graph, record, &["Version", "version"]) {
        Some(NrbfValue::Int32(value)) => *value,
        _ => return Err("ASTRA_EMU_GARBRO_VERSION".into()),
    };
    if !(0..=2).contains(&version) {
        return Err("ASTRA_EMU_GARBRO_VERSION".into());
    }
    let arc_keys = object_member(graph, record, &["ArcKeys", "arc_keys"])
        .ok_or("ASTRA_EMU_GARBRO_ARC_KEYS")?;
    let arc_entries = dictionary_entries(graph, arc_keys)?;
    if arc_entries.len() > REQUIRED_ARCHIVE_ROLES.len()
        || arc_entries.iter().any(|(role, _)| {
            !REQUIRED_ARCHIVE_ROLES
                .iter()
                .any(|expected| role.eq_ignore_ascii_case(expected))
        })
    {
        return Err("ASTRA_EMU_GARBRO_ROLE_SET".into());
    }
    let type_entries = object_member(graph, record, &["TypeKeys", "type_keys"])
        .map(|value| dictionary_entries(graph, value))
        .transpose()?
        .unwrap_or_default();
    if type_entries.len() > 4 || type_entries.iter().any(|(key, value)| !["png", "ogg", "sc", "avi"].iter().any(|expected| key.eq_ignore_ascii_case(expected)) || !matches!(graph.dereference(value), Ok(NrbfValue::String(value)) if value.len() <= 1024)) {
        return Err("ASTRA_EMU_GARBRO_TYPE_KEY_SET".into());
    }
    let passwords = type_entries
        .into_iter()
        .map(|(key, value)| match graph.dereference(value) {
            Ok(NrbfValue::String(value)) => Ok((key.to_ascii_lowercase(), value.to_owned())),
            _ => Err("ASTRA_EMU_GARBRO_TYPE_KEY_VALUE"),
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let mut roles = BTreeMap::new();
    for role in REQUIRED_ARCHIVE_ROLES {
        let value = arc_entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(role))
            .map(|(_, value)| *value)
            .ok_or("ASTRA_EMU_GARBRO_ROLE_MISSING")?;
        let value = graph
            .dereference(value)
            .map_err(|_| "ASTRA_EMU_GARBRO_REFERENCE")?;
        if !matches!(value, NrbfValue::Object(object) if object.class.ends_with("PazKey")) {
            return Err("ASTRA_EMU_GARBRO_ROLE_TYPE".into());
        }
        let index_key = array_bytes(
            graph,
            object_member(graph, value, &["IndexKey", "index_key"])
                .ok_or("ASTRA_EMU_GARBRO_INDEX_KEY")?,
        )?;
        let data_value = object_member(graph, value, &["DataKey", "data_key"])
            .ok_or("ASTRA_EMU_GARBRO_DATA_KEY")?;
        let data_key = if role == "mov" && matches!(data_value, NrbfValue::Null) {
            Vec::new()
        } else {
            array_bytes(graph, data_value)?
        };
        if !(4..=56).contains(&index_key.len())
            || (role != "mov" && !(4..=56).contains(&data_key.len()))
            || data_key.len() > 56
        {
            return Err("ASTRA_EMU_GARBRO_KEY_SIZE".into());
        }
        roles.insert(
            role.into(),
            ImportedRole {
                index_key,
                data_key,
                type_keys: passwords.clone(),
                version,
            },
        );
    }
    Ok(roles)
}

fn array_bytes(
    graph: &NrbfGraph,
    value: &NrbfValue,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let NrbfValue::Array(values) = graph
        .dereference(value)
        .map_err(|_| "ASTRA_EMU_GARBRO_REFERENCE")?
    else {
        return Err("ASTRA_EMU_GARBRO_KEY_TYPE".into());
    };
    values
        .iter()
        .map(|value| match graph.dereference(value) {
            Ok(NrbfValue::Byte(value)) => Ok(*value),
            Ok(NrbfValue::UInt16(value)) => {
                u8::try_from(*value).map_err(|_| "ASTRA_EMU_GARBRO_KEY_BYTE".into())
            }
            _ => Err("ASTRA_EMU_GARBRO_KEY_BYTE".into()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{import, PATCH_NAME};

    #[test]
    fn existing_output_blocks_before_formats_are_read() {
        let game = tempfile::tempdir().unwrap();
        std::fs::write(game.path().join(PATCH_NAME), b"private").unwrap();
        let error = import(&game.path().join("missing.dat"), "title", game.path()).unwrap_err();
        assert_eq!(error.to_string(), "ASTRA_EMU_GARBRO_OUTPUT_EXISTS");
    }
}
