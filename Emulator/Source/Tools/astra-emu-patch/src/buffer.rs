//! Borrow Luau buffers only while its VM is locked. Native operations must not
//! call back into Lua or retain the borrowed slice; no intermediate to_vec copy.
use crate::{
    storage::{TaskFile, MAX_EXACT_INTEGER},
    PatchError, PatchErrorKind,
};
use astra_byte_source::{ByteRange, DEFAULT_MAX_RANGE_BYTES};
use mlua::{Buffer, Lua, Value};

pub(crate) const MAX_NATIVE_BYTES: usize = 64 * 1024 * 1024;

pub(crate) struct BoundedWriter {
    bytes: Vec<u8>,
}

impl BoundedWriter {
    pub(crate) fn new() -> Self {
        Self { bytes: Vec::new() }
    }
    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

impl std::io::Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let length = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|length| *length <= MAX_NATIVE_BYTES)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::FileTooLarge,
                    "native block exceeds 64MiB",
                )
            })?;
        if length > self.bytes.capacity() {
            let capacity = length
                .max(self.bytes.capacity().saturating_mul(2))
                .min(MAX_NATIVE_BYTES);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|cause| std::io::Error::new(std::io::ErrorKind::OutOfMemory, cause))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn integer(value: Value) -> mlua::Result<u64> {
    let number = match value {
        Value::Integer(number) => number as f64,
        Value::Number(number) => number,
        _ => {
            return Err(mlua::Error::external(PatchError::new(
                PatchErrorKind::Bounds,
                "expected a nonnegative exact integer",
            )))
        }
    };
    if !number.is_finite()
        || number < 0.0
        || number > MAX_EXACT_INTEGER as f64
        || number.fract() != 0.0
    {
        return Err(mlua::Error::external(PatchError::new(
            PatchErrorKind::Bounds,
            "expected a nonnegative exact integer",
        )));
    }
    Ok(number as u64)
}

pub(crate) fn bounded(length: usize) -> Result<(), PatchError> {
    if length > MAX_NATIVE_BYTES {
        Err(PatchError::new(
            PatchErrorKind::Bounds,
            "native block exceeds 64MiB",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn output(lua: &Lua, bytes: &[u8]) -> mlua::Result<Buffer> {
    bounded(bytes.len()).map_err(mlua::Error::external)?;
    lua.create_buffer(bytes)
}

/// `operation` is native-only and returns owned data; it must not call Lua,
/// mutate this buffer through another handle, or retain any borrowed bytes.
pub(crate) fn input<R>(
    lua: &Lua,
    buffer: Buffer,
    operation: impl for<'bytes> FnOnce(&'bytes [u8]) -> Result<R, PatchError>,
) -> mlua::Result<R> {
    let mut result = None;
    // SAFETY: a typed Buffer is rooted as the sole argument for the duration of
    // exec_raw. The Lua lock prevents GC/reallocation and the native closure
    // never enters Lua. The slice cannot outlive this call's higher-rank borrow.
    unsafe {
        lua.exec_raw::<()>(buffer, |state| {
            let mut length = 0;
            let pointer = mlua::ffi::lua_tobuffer(state, -1, &mut length).cast::<u8>();
            let bytes = if length == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(pointer, length)
            };
            result = Some(operation(bytes));
            mlua::ffi::lua_pop(state, 1);
        })?;
    }
    result
        .expect("native buffer operation executed")
        .map_err(mlua::Error::external)
}

/// Same native-only borrowing rule as `input`; either buffer may alias the
/// other because both slices are immutable.
pub(crate) fn input2<R>(
    lua: &Lua,
    first: Buffer,
    second: Buffer,
    operation: impl for<'bytes> FnOnce(&'bytes [u8], &'bytes [u8]) -> Result<R, PatchError>,
) -> mlua::Result<R> {
    let mut result = None;
    // SAFETY: both arguments stay rooted and immutable under the Lua lock.
    unsafe {
        lua.exec_raw::<()>((first, second), |state| {
            let mut first_length = 0;
            let mut second_length = 0;
            let first_pointer = mlua::ffi::lua_tobuffer(state, -2, &mut first_length).cast::<u8>();
            let second_pointer =
                mlua::ffi::lua_tobuffer(state, -1, &mut second_length).cast::<u8>();
            let first = if first_length == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(first_pointer, first_length)
            };
            let second = if second_length == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(second_pointer, second_length)
            };
            result = Some(operation(first, second));
            mlua::ffi::lua_pop(state, 2);
        })?;
    }
    result
        .expect("native buffer operation executed")
        .map_err(mlua::Error::external)
}

pub(crate) fn read(
    lua: &Lua,
    file: &mut TaskFile,
    offset: u64,
    length: u64,
) -> mlua::Result<Buffer> {
    ByteRange {
        offset,
        len: length,
    }
    .validate(
        file.size().map_err(mlua::Error::external)?,
        DEFAULT_MAX_RANGE_BYTES,
    )
    .map_err(|cause| {
        mlua::Error::external(PatchError::with_source(
            PatchErrorKind::Bounds,
            "file read exceeds its range",
            cause,
        ))
    })?;
    let length = usize::try_from(length).map_err(|cause| {
        mlua::Error::external(PatchError::with_source(
            PatchErrorKind::Bounds,
            "file read length overflow",
            cause,
        ))
    })?;
    let mut result = Ok(());
    // SAFETY: lua_newbuffer is inside mlua's protected allocation boundary;
    // after allocation only native file I/O touches the newly owned slice.
    let buffer = unsafe {
        lua.exec_raw::<Buffer>((), |state| {
            let pointer = mlua::ffi::lua_newbuffer(state, length).cast::<u8>();
            let bytes = if length == 0 {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(pointer, length)
            };
            result = file.read(offset, bytes);
        })?
    };
    result.map_err(mlua::Error::external)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_buffers_preserve_zeroes_and_aliased_inputs() {
        let lua = Lua::new();
        let buffer = lua.create_buffer([0, 1, 255, 0]).unwrap();
        let count = input2(&lua, buffer.clone(), buffer.clone(), |first, second| {
            assert_eq!(first, &[0, 1, 255, 0]);
            assert_eq!(second, first);
            Ok(first.len())
        })
        .unwrap();
        assert_eq!(count, 4);
        assert!(input(&lua, lua.create_buffer([]).unwrap(), |bytes| Ok(
            bytes.is_empty()
        ))
        .unwrap());
        assert_eq!(buffer.to_vec(), [0, 1, 255, 0]);
    }

    #[test]
    fn integers_reject_coercion_fraction_and_unsafe_range() {
        for value in [
            Value::Number(-1.0),
            Value::Number(f64::NAN),
            Value::Number(f64::INFINITY),
            Value::Number(0.5),
            Value::Number((MAX_EXACT_INTEGER + 1) as f64),
            Value::Boolean(true),
        ] {
            assert!(integer(value).is_err());
        }
        assert_eq!(integer(Value::Number(0.0)).unwrap(), 0);
        assert_eq!(
            integer(Value::Number(MAX_EXACT_INTEGER as f64)).unwrap(),
            MAX_EXACT_INTEGER
        );
    }
}
