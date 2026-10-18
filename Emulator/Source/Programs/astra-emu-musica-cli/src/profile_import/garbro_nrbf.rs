use std::collections::BTreeMap;

mod model;

use model::dereference_nodes;
pub(super) use model::{NrbfError, NrbfGraph, NrbfObject, NrbfValue};

const MAX_NODES: usize = 1_000_000;
const MAX_DEPTH: usize = 128;
const MAX_ARRAY_LENGTH: usize = 1_000_000;
const MAX_STRING_BYTES: usize = 16 * 1024 * 1024;
const MAX_MEMBERS: usize = 100_000;

#[derive(Debug, Clone)]
struct ClassMetadata {
    class: String,
    library: Option<String>,
    member_names: Vec<String>,
    member_types: Vec<MemberType>,
}

#[derive(Debug, Clone)]
enum MemberType {
    Untyped,
    Primitive(PrimitiveType),
    String,
    Object,
    SystemClass,
    Class { library_id: u32 },
    Array,
    PrimitiveArray,
}

#[derive(Debug, Clone, Copy)]
enum PrimitiveType {
    Boolean,
    Byte,
    Char,
    Decimal,
    Double,
    Int16,
    Int32,
    Int64,
    SByte,
    Single,
    TimeSpan,
    DateTime,
    UInt16,
    UInt32,
    UInt64,
    Null,
    String,
}

#[derive(Debug, Clone)]
enum ExpectedValue {
    String,
    Array,
}

impl NrbfGraph {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, NrbfError> {
        Parser::new(bytes).parse()
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    cursor: usize,
    root_id: i32,
    libraries: BTreeMap<u32, String>,
    metadata: BTreeMap<i32, ClassMetadata>,
    nodes: BTreeMap<i32, NrbfValue>,
    expected: Vec<(i32, ExpectedValue)>,
    top_level_references: Vec<i32>,
}

impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            cursor: 0,
            root_id: 0,
            libraries: BTreeMap::new(),
            metadata: BTreeMap::new(),
            nodes: BTreeMap::new(),
            expected: Vec::new(),
            top_level_references: Vec::new(),
        }
    }

    fn parse(mut self) -> Result<NrbfGraph, NrbfError> {
        if self.byte()? != 0 {
            return Err(NrbfError("ASTRA_EMU_NRBF_HEADER_RECORD"));
        }
        self.root_id = self.object_id()?;
        let _header_id = self.i32()?;
        if self.i32()? != 1 || self.i32()? != 0 {
            return Err(NrbfError("ASTRA_EMU_NRBF_VERSION"));
        }
        let mut ended = false;
        while self.cursor < self.bytes.len() {
            if self.peek()? == 11 {
                self.cursor += 1;
                ended = true;
                break;
            }
            match self.record_value(0)? {
                NrbfValue::Ref(id) => self.top_level_references.push(id),
                _ => return Err(NrbfError("ASTRA_EMU_NRBF_TOP_LEVEL_VALUE")),
            }
        }
        if !ended || self.cursor != self.bytes.len() {
            return Err(NrbfError("ASTRA_EMU_NRBF_TRAILING_DATA"));
        }
        if !self.nodes.contains_key(&self.root_id) {
            return Err(NrbfError("ASTRA_EMU_NRBF_ROOT_MISSING"));
        }
        self.validate_references()?;
        self.validate_expected()?;
        Ok(NrbfGraph {
            root_id: self.root_id,
            nodes: self.nodes,
        })
    }

    fn record_value(&mut self, depth: usize) -> Result<NrbfValue, NrbfError> {
        self.check_depth(depth)?;
        while self.peek()? == 12 {
            self.cursor += 1;
            let id = self.library_id()?;
            let name = self.string()?;
            if self.libraries.insert(id, name).is_some() {
                return Err(NrbfError("ASTRA_EMU_NRBF_LIBRARY_DUPLICATE"));
            }
        }
        match self.byte()? {
            1 => self.class_with_id(depth + 1),
            kind @ 2..=5 => self.class_with_metadata(kind, depth + 1),
            6 => self.object_string(),
            7 => self.binary_array(depth + 1),
            8 => {
                let primitive = self.primitive_type()?;
                self.primitive(primitive)
            }
            9 => Ok(NrbfValue::Ref(self.object_id()?)),
            10 => Ok(NrbfValue::Null),
            15 => self.single_primitive_array(depth + 1),
            16 => self.single_array(MemberType::Object, depth + 1),
            17 => self.single_array(MemberType::String, depth + 1),
            _ => Err(NrbfError("ASTRA_EMU_NRBF_RECORD_UNSUPPORTED")),
        }
    }

    fn class_with_metadata(&mut self, kind: u8, depth: usize) -> Result<NrbfValue, NrbfError> {
        let object_id = self.object_id()?;
        let class = self.string()?;
        let member_count = self.length(MAX_MEMBERS)?;
        let mut member_names = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            member_names.push(self.string()?);
        }
        let member_types = if matches!(kind, 4 | 5) {
            self.member_types(member_count)?
        } else {
            vec![MemberType::Untyped; member_count]
        };
        let library = if matches!(kind, 3 | 5) {
            let id = self.library_id()?;
            Some(
                self.libraries
                    .get(&id)
                    .cloned()
                    .ok_or(NrbfError("ASTRA_EMU_NRBF_LIBRARY_MISSING"))?,
            )
        } else {
            None
        };
        let metadata = ClassMetadata {
            class,
            library,
            member_names,
            member_types,
        };
        if self.metadata.insert(object_id, metadata.clone()).is_some() {
            return Err(NrbfError("ASTRA_EMU_NRBF_METADATA_DUPLICATE"));
        }
        self.class_members(object_id, metadata, depth)
    }

    fn class_with_id(&mut self, depth: usize) -> Result<NrbfValue, NrbfError> {
        let object_id = self.object_id()?;
        let metadata_id = self.object_id()?;
        if object_id == metadata_id {
            return Err(NrbfError("ASTRA_EMU_NRBF_METADATA_ID"));
        }
        let metadata = self
            .metadata
            .get(&metadata_id)
            .cloned()
            .ok_or(NrbfError("ASTRA_EMU_NRBF_METADATA_MISSING"))?;
        self.class_members(object_id, metadata, depth)
    }

    fn class_members(
        &mut self,
        object_id: i32,
        metadata: ClassMetadata,
        depth: usize,
    ) -> Result<NrbfValue, NrbfError> {
        let values = self.sequence(&metadata.member_types, depth)?;
        let members = metadata
            .member_names
            .into_iter()
            .zip(values)
            .collect::<BTreeMap<_, _>>();
        self.insert_node(
            object_id,
            NrbfValue::Object(NrbfObject {
                class: metadata.class,
                library: metadata.library,
                members,
            }),
        )?;
        Ok(NrbfValue::Ref(object_id))
    }

    fn object_string(&mut self) -> Result<NrbfValue, NrbfError> {
        let id = self.object_id()?;
        let value = NrbfValue::String(self.string()?);
        self.insert_node(id, value)?;
        Ok(NrbfValue::Ref(id))
    }

    fn single_array(
        &mut self,
        member_type: MemberType,
        depth: usize,
    ) -> Result<NrbfValue, NrbfError> {
        let id = self.object_id()?;
        let length = self.length(MAX_ARRAY_LENGTH)?;
        let types = vec![member_type; length];
        let values = self.sequence(&types, depth)?;
        self.insert_node(id, NrbfValue::Array(values))?;
        Ok(NrbfValue::Ref(id))
    }

    fn single_primitive_array(&mut self, _depth: usize) -> Result<NrbfValue, NrbfError> {
        let id = self.object_id()?;
        let length = self.length(MAX_ARRAY_LENGTH)?;
        let primitive = self.primitive_type()?;
        let mut values = Vec::with_capacity(length);
        for _ in 0..length {
            values.push(self.primitive(primitive)?);
        }
        self.insert_node(id, NrbfValue::Array(values))?;
        Ok(NrbfValue::Ref(id))
    }

    fn binary_array(&mut self, depth: usize) -> Result<NrbfValue, NrbfError> {
        let id = self.object_id()?;
        let array_kind = self.byte()?;
        if array_kind > 5 {
            return Err(NrbfError("ASTRA_EMU_NRBF_ARRAY_KIND"));
        }
        let rank = self.length(32)?;
        if rank == 0 {
            return Err(NrbfError("ASTRA_EMU_NRBF_ARRAY_RANK"));
        }
        let mut length = 1usize;
        for _ in 0..rank {
            length = length
                .checked_mul(self.length(MAX_ARRAY_LENGTH)?)
                .filter(|value| *value <= MAX_ARRAY_LENGTH)
                .ok_or(NrbfError("ASTRA_EMU_NRBF_ARRAY_LENGTH"))?;
        }
        if array_kind >= 3 {
            for _ in 0..rank {
                let _ = self.i32()?;
            }
        }
        let member_type = self.member_type_header()?;
        let types = vec![member_type; length];
        let values = self.sequence(&types, depth)?;
        self.insert_node(id, NrbfValue::Array(values))?;
        Ok(NrbfValue::Ref(id))
    }

    fn sequence(
        &mut self,
        types: &[MemberType],
        depth: usize,
    ) -> Result<Vec<NrbfValue>, NrbfError> {
        let mut values = Vec::with_capacity(types.len());
        let mut pending_nulls = 0usize;
        for member_type in types {
            if pending_nulls > 0 {
                values.push(NrbfValue::Null);
                pending_nulls -= 1;
                continue;
            }
            if !matches!(member_type, MemberType::Primitive(_)) {
                match self.peek()? {
                    13 => {
                        self.cursor += 1;
                        pending_nulls = usize::from(self.byte()?);
                    }
                    14 => {
                        self.cursor += 1;
                        pending_nulls = self.length(MAX_ARRAY_LENGTH)?;
                    }
                    _ => {}
                }
                if pending_nulls > 0 {
                    values.push(NrbfValue::Null);
                    pending_nulls -= 1;
                    continue;
                }
            }
            values.push(self.member_value(member_type, depth)?);
        }
        if pending_nulls != 0 {
            return Err(NrbfError("ASTRA_EMU_NRBF_NULL_RUN"));
        }
        Ok(values)
    }

    fn member_value(
        &mut self,
        member_type: &MemberType,
        depth: usize,
    ) -> Result<NrbfValue, NrbfError> {
        if let MemberType::Primitive(primitive) = member_type {
            return self.primitive(*primitive);
        }
        let value = self.record_value(depth)?;
        let expected = match member_type {
            MemberType::String => Some(ExpectedValue::String),
            MemberType::SystemClass => None,
            MemberType::Class { library_id, .. } => {
                if !self.libraries.contains_key(library_id) {
                    return Err(NrbfError("ASTRA_EMU_NRBF_LIBRARY_MISSING"));
                }
                None
            }
            MemberType::Array | MemberType::PrimitiveArray => Some(ExpectedValue::Array),
            MemberType::Object => None,
            MemberType::Untyped | MemberType::Primitive(_) => None,
        };
        if let (Some(expected), NrbfValue::Ref(id)) = (&expected, &value) {
            self.expected.push((*id, expected.clone()));
        } else if !matches!(value, NrbfValue::Null) {
            if let Some(expected) = &expected {
                let valid = matches!(
                    (expected, &value),
                    (ExpectedValue::String, NrbfValue::String(_))
                        | (ExpectedValue::Array, NrbfValue::Array(_))
                );
                if !valid {
                    return Err(NrbfError(match expected {
                        ExpectedValue::String => "ASTRA_EMU_NRBF_MEMBER_TYPE_STRING",
                        ExpectedValue::Array => "ASTRA_EMU_NRBF_MEMBER_TYPE_ARRAY",
                    }));
                }
            }
        }
        Ok(value)
    }

    fn member_types(&mut self, count: usize) -> Result<Vec<MemberType>, NrbfError> {
        let mut headers = Vec::with_capacity(count);
        for _ in 0..count {
            headers.push(self.byte()?);
        }
        headers
            .into_iter()
            .map(|header| self.member_type_from_header(header))
            .collect()
    }

    fn member_type_header(&mut self) -> Result<MemberType, NrbfError> {
        let header = self.byte()?;
        self.member_type_from_header(header)
    }

    fn member_type_from_header(&mut self, header: u8) -> Result<MemberType, NrbfError> {
        match header {
            0 => Ok(MemberType::Primitive(self.primitive_type()?)),
            1 => Ok(MemberType::String),
            2 => Ok(MemberType::Object),
            3 => {
                let _ = self.string()?;
                Ok(MemberType::SystemClass)
            }
            4 => {
                let _ = self.string()?;
                Ok(MemberType::Class {
                    library_id: self.library_id()?,
                })
            }
            5 | 6 => Ok(MemberType::Array),
            7 => {
                let _ = self.primitive_type()?;
                Ok(MemberType::PrimitiveArray)
            }
            _ => Err(NrbfError("ASTRA_EMU_NRBF_BINARY_TYPE")),
        }
    }

    fn primitive_type(&mut self) -> Result<PrimitiveType, NrbfError> {
        match self.byte()? {
            1 => Ok(PrimitiveType::Boolean),
            2 => Ok(PrimitiveType::Byte),
            3 => Ok(PrimitiveType::Char),
            5 => Ok(PrimitiveType::Decimal),
            6 => Ok(PrimitiveType::Double),
            7 => Ok(PrimitiveType::Int16),
            8 => Ok(PrimitiveType::Int32),
            9 => Ok(PrimitiveType::Int64),
            10 => Ok(PrimitiveType::SByte),
            11 => Ok(PrimitiveType::Single),
            12 => Ok(PrimitiveType::TimeSpan),
            13 => Ok(PrimitiveType::DateTime),
            14 => Ok(PrimitiveType::UInt16),
            15 => Ok(PrimitiveType::UInt32),
            16 => Ok(PrimitiveType::UInt64),
            17 => Ok(PrimitiveType::Null),
            18 => Ok(PrimitiveType::String),
            _ => Err(NrbfError("ASTRA_EMU_NRBF_PRIMITIVE_TYPE")),
        }
    }

    fn primitive(&mut self, primitive: PrimitiveType) -> Result<NrbfValue, NrbfError> {
        match primitive {
            PrimitiveType::Boolean => match self.byte()? {
                0 => Ok(NrbfValue::Boolean(false)),
                1 => Ok(NrbfValue::Boolean(true)),
                _ => Err(NrbfError("ASTRA_EMU_NRBF_BOOLEAN")),
            },
            PrimitiveType::Byte => Ok(NrbfValue::Byte(self.byte()?)),
            PrimitiveType::Int32 => Ok(NrbfValue::Int32(self.i32()?)),
            PrimitiveType::UInt16 => Ok(NrbfValue::UInt16(self.u16()?)),
            PrimitiveType::Null => Ok(NrbfValue::Null),
            PrimitiveType::String | PrimitiveType::Decimal => Ok(NrbfValue::String(self.string()?)),
            PrimitiveType::Char => {
                let first = self.byte()?;
                let width = if first < 0x80 {
                    1
                } else if first & 0xe0 == 0xc0 {
                    2
                } else if first & 0xf0 == 0xe0 {
                    3
                } else if first & 0xf8 == 0xf0 {
                    4
                } else {
                    return Err(NrbfError("ASTRA_EMU_NRBF_CHAR"));
                };
                let mut encoded = [0u8; 4];
                encoded[0] = first;
                encoded[1..width].copy_from_slice(self.take(width - 1)?);
                let value = std::str::from_utf8(&encoded[..width])
                    .map_err(|_| NrbfError("ASTRA_EMU_NRBF_CHAR"))?;
                if value.chars().count() != 1 {
                    return Err(NrbfError("ASTRA_EMU_NRBF_CHAR"));
                }
                Ok(NrbfValue::Number)
            }
            PrimitiveType::Int16 => {
                let _ = self.take(2)?;
                Ok(NrbfValue::Number)
            }
            PrimitiveType::Double
            | PrimitiveType::Int64
            | PrimitiveType::TimeSpan
            | PrimitiveType::DateTime
            | PrimitiveType::UInt64 => {
                let _ = self.take(8)?;
                Ok(NrbfValue::Number)
            }
            PrimitiveType::SByte => {
                let _ = self.byte()?;
                Ok(NrbfValue::Number)
            }
            PrimitiveType::Single | PrimitiveType::UInt32 => {
                let _ = self.take(4)?;
                Ok(NrbfValue::Number)
            }
        }
    }

    fn validate_references(&self) -> Result<(), NrbfError> {
        if self
            .top_level_references
            .iter()
            .any(|id| !self.nodes.contains_key(id))
        {
            return Err(NrbfError("ASTRA_EMU_NRBF_REFERENCE_MISSING"));
        }
        let mut stack = self.nodes.values().collect::<Vec<_>>();
        let mut visited = 0usize;
        while let Some(value) = stack.pop() {
            visited += 1;
            if visited > MAX_NODES * 4 {
                return Err(NrbfError("ASTRA_EMU_NRBF_GRAPH_LIMIT"));
            }
            match value {
                NrbfValue::Ref(id) => {
                    if !self.nodes.contains_key(id) {
                        return Err(NrbfError("ASTRA_EMU_NRBF_REFERENCE_MISSING"));
                    }
                }
                NrbfValue::Array(values) => stack.extend(values),
                NrbfValue::Object(object) => stack.extend(object.members.values()),
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_expected(&self) -> Result<(), NrbfError> {
        for (id, expected) in &self.expected {
            let reference = NrbfValue::Ref(*id);
            let value = dereference_nodes(&self.nodes, &reference)?;
            let valid = matches!(
                (expected, value),
                (ExpectedValue::String, NrbfValue::String(_))
                    | (ExpectedValue::Array, NrbfValue::Array(_))
            );
            if !valid {
                return Err(NrbfError("ASTRA_EMU_NRBF_REFERENCE_TYPE"));
            }
        }
        Ok(())
    }

    fn insert_node(&mut self, id: i32, value: NrbfValue) -> Result<(), NrbfError> {
        if self.nodes.len() == MAX_NODES || self.nodes.insert(id, value).is_some() {
            return Err(NrbfError("ASTRA_EMU_NRBF_OBJECT_DUPLICATE"));
        }
        Ok(())
    }

    fn object_id(&mut self) -> Result<i32, NrbfError> {
        let value = self.i32()?;
        if value == 0 {
            Err(NrbfError("ASTRA_EMU_NRBF_OBJECT_ID"))
        } else {
            Ok(value)
        }
    }

    fn library_id(&mut self) -> Result<u32, NrbfError> {
        u32::try_from(self.i32()?)
            .ok()
            .filter(|value| *value != 0)
            .ok_or(NrbfError("ASTRA_EMU_NRBF_LIBRARY_ID"))
    }

    fn length(&mut self, maximum: usize) -> Result<usize, NrbfError> {
        usize::try_from(self.i32()?)
            .ok()
            .filter(|value| *value <= maximum)
            .ok_or(NrbfError("ASTRA_EMU_NRBF_LENGTH"))
    }

    fn string(&mut self) -> Result<String, NrbfError> {
        let mut length = 0u32;
        for shift in (0..=28).step_by(7) {
            let byte = self.byte()?;
            length |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                let length = usize::try_from(length)
                    .ok()
                    .filter(|value| *value <= MAX_STRING_BYTES)
                    .ok_or(NrbfError("ASTRA_EMU_NRBF_STRING_LENGTH"))?;
                return std::str::from_utf8(self.take(length)?)
                    .map(str::to_owned)
                    .map_err(|_| NrbfError("ASTRA_EMU_NRBF_STRING_UTF8"));
            }
        }
        Err(NrbfError("ASTRA_EMU_NRBF_STRING_PREFIX"))
    }

    fn check_depth(&self, depth: usize) -> Result<(), NrbfError> {
        if depth > MAX_DEPTH {
            Err(NrbfError("ASTRA_EMU_NRBF_DEPTH"))
        } else {
            Ok(())
        }
    }

    fn peek(&self) -> Result<u8, NrbfError> {
        self.bytes
            .get(self.cursor)
            .copied()
            .ok_or(NrbfError("ASTRA_EMU_NRBF_EOF"))
    }

    fn byte(&mut self) -> Result<u8, NrbfError> {
        let value = self.peek()?;
        self.cursor += 1;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, NrbfError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| NrbfError("ASTRA_EMU_NRBF_EOF"))?,
        ))
    }

    fn i32(&mut self) -> Result<i32, NrbfError> {
        Ok(i32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| NrbfError("ASTRA_EMU_NRBF_EOF"))?,
        ))
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], NrbfError> {
        let end = self
            .cursor
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(NrbfError("ASTRA_EMU_NRBF_EOF"))?;
        let value = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
