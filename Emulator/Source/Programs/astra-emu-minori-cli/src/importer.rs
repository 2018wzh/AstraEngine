use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::Path,
};

use astra_emu_minori::{
    MinoriProfile, MinoriRolePrivateProfile, MINORI_PROFILE_FILE, MINORI_PROFILE_SCHEMA,
    REQUIRED_ARCHIVE_ROLES,
};
use flate2::read::ZlibDecoder;

use crate::garbro_nrbf::{NrbfGraph, NrbfValue};

const PROFILE_NAME: &str = MINORI_PROFILE_FILE;
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
    let profile_path = game_dir.join(PROFILE_NAME);
    if fs::symlink_metadata(&profile_path).is_ok() {
        return Err("ASTRA_EMU_GARBRO_OUTPUT_EXISTS".into());
    }
    const MAX_FORMATS_BYTES: u64 = 256 * 1024 * 1024;
    let mut bytes = Vec::new();
    File::open(formats)?
        .take(MAX_FORMATS_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FORMATS_BYTES {
        return Err("ASTRA_EMU_GARBRO_SIZE".into());
    }
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
    let profile = MinoriProfile {
        schema: MINORI_PROFILE_SCHEMA.into(),
        paz_version: u8::try_from(version).map_err(|_| "ASTRA_EMU_GARBRO_VERSION")?,
        index_size_xor: 0,
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
    let profile_bytes = serde_json::to_vec_pretty(&profile)?;
    if profile_bytes.len() > 1024 * 1024 {
        return Err("ASTRA_EMU_GARBRO_PROFILE_SIZE".into());
    }
    crate::private_output::write_new_private(&profile_path, &profile_bytes)?;
    println!("{{\"schema\":\"astra.emu.minori.garbro_import.v3\",\"status\":\"passed\"}}");
    Ok(())
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
    use super::{import, PROFILE_NAME};

    #[test]
    fn existing_output_blocks_before_formats_are_read() {
        let game = tempfile::tempdir().unwrap();
        std::fs::write(game.path().join(PROFILE_NAME), b"private").unwrap();
        let error = import(&game.path().join("missing.dat"), "title", game.path()).unwrap_err();
        assert_eq!(error.to_string(), "ASTRA_EMU_GARBRO_OUTPUT_EXISTS");
    }
}

#[cfg(test)]
#[path = "importer_tests.rs"]
mod conversion_tests;
