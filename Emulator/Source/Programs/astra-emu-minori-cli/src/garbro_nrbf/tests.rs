use super::{NrbfGraph, NrbfValue};

fn header(root_id: i32) -> Vec<u8> {
    let mut bytes = vec![0];
    bytes.extend_from_slice(&root_id.to_le_bytes());
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    bytes.extend_from_slice(&1i32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes
}

#[test]
fn resolves_forward_referenced_root_string() {
    let bytes = [
        0, 1, 0, 0, 0, 255, 255, 255, 255, 1, 0, 0, 0, 0, 0, 0, 0, 6, 1, 0, 0, 0, 2, b'o', b'k', 11,
    ];
    let graph = NrbfGraph::parse(&bytes).unwrap();
    assert_eq!(graph.root().unwrap(), &NrbfValue::String("ok".into()));
}

#[test]
fn duplicate_object_id_is_blocking() {
    let bytes = [
        0, 1, 0, 0, 0, 255, 255, 255, 255, 1, 0, 0, 0, 0, 0, 0, 0, 6, 1, 0, 0, 0, 1, b'a', 6, 1, 0,
        0, 0, 1, b'b', 11,
    ];
    assert!(NrbfGraph::parse(&bytes).is_err());
}

#[test]
fn resolves_forward_reference_with_negative_object_id() {
    let mut bytes = header(1);
    bytes.extend_from_slice(&[
        4, // SystemClassWithMembersAndTypes
        1, 0, 0, 0, // object id
        4, b'R', b'o', b'o', b't', // class name
        1, 0, 0, 0, // member count
        5, b'v', b'a', b'l', b'u', b'e', // member name
        1,    // member type: string
        9, 254, 255, 255, 255, // forward reference to -2
        6, 254, 255, 255, 255, 2, b'o', b'k', // referenced string
        11,
    ]);
    let graph = NrbfGraph::parse(&bytes).unwrap();
    let NrbfValue::Object(root) = graph.root().unwrap() else {
        panic!("root must be an object");
    };
    assert_eq!(
        graph
            .dereference(root.members.get("value").unwrap())
            .unwrap(),
        &NrbfValue::String("ok".into())
    );
}

#[test]
fn unresolved_forward_reference_is_blocking() {
    let mut bytes = header(1);
    bytes.extend_from_slice(&[6, 1, 0, 0, 0, 0, 9, 2, 0, 0, 0, 11]);
    let error = NrbfGraph::parse(&bytes).err().unwrap();
    assert_eq!(error.code(), "ASTRA_EMU_NRBF_REFERENCE_MISSING");
}

#[test]
fn trailing_data_is_blocking() {
    let mut bytes = header(1);
    bytes.extend_from_slice(&[6, 1, 0, 0, 0, 0, 11, 0]);
    let error = NrbfGraph::parse(&bytes).err().unwrap();
    assert_eq!(error.code(), "ASTRA_EMU_NRBF_TRAILING_DATA");
}
