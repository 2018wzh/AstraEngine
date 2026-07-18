use std::collections::{BTreeMap, BTreeSet};

const MAX_REFERENCE_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NrbfValue {
    Null,
    Boolean(bool),
    Byte(u8),
    Int32(i32),
    UInt16(u16),
    String(String),
    Number,
    Array(Vec<NrbfValue>),
    Object(NrbfObject),
    Ref(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NrbfObject {
    pub(crate) class: String,
    pub(crate) library: Option<String>,
    pub(crate) members: BTreeMap<String, NrbfValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NrbfError(pub(super) &'static str);

impl NrbfError {
    pub(crate) fn code(self) -> &'static str {
        self.0
    }
}

pub(crate) struct NrbfGraph {
    pub(super) root_id: i32,
    pub(super) nodes: BTreeMap<i32, NrbfValue>,
}

impl NrbfGraph {
    pub(crate) fn root(&self) -> Result<&NrbfValue, NrbfError> {
        self.nodes
            .get(&self.root_id)
            .ok_or(NrbfError("ASTRA_EMU_NRBF_ROOT_MISSING"))
    }

    pub(crate) fn dereference<'a>(
        &'a self,
        value: &'a NrbfValue,
    ) -> Result<&'a NrbfValue, NrbfError> {
        dereference_nodes(&self.nodes, value)
    }
}

pub(super) fn dereference_nodes<'a>(
    nodes: &'a BTreeMap<i32, NrbfValue>,
    mut value: &'a NrbfValue,
) -> Result<&'a NrbfValue, NrbfError> {
    let mut visited = BTreeSet::new();
    for _ in 0..=MAX_REFERENCE_DEPTH {
        let NrbfValue::Ref(id) = value else {
            return Ok(value);
        };
        if !visited.insert(*id) {
            return Err(NrbfError("ASTRA_EMU_NRBF_REFERENCE_CYCLE"));
        }
        value = nodes
            .get(id)
            .ok_or(NrbfError("ASTRA_EMU_NRBF_REFERENCE_MISSING"))?;
    }
    Err(NrbfError("ASTRA_EMU_NRBF_REFERENCE_DEPTH"))
}
