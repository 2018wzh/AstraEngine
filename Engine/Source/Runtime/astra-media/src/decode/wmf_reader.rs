use std::{
    io::{Read, Seek, SeekFrom},
    ptr,
    sync::Mutex,
};

use windows::{
    core::{implement, HRESULT},
    Win32::System::Com::{
        ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE, STATFLAG, STATSTG, STGC, STGM,
        STREAM_SEEK, STREAM_SEEK_CUR, STREAM_SEEK_END, STREAM_SEEK_SET,
    },
};

use crate::IncrementalReadSeek;

const S_OK: HRESULT = HRESULT(0);
const S_FALSE: HRESULT = HRESULT(1);
const STG_E_INVALIDFUNCTION: HRESULT = HRESULT(0x8003_0001_u32 as i32);
const STG_E_ACCESSDENIED: HRESULT = HRESULT(0x8003_0005_u32 as i32);
const STG_E_SEEKERROR: HRESULT = HRESULT(0x8003_0019_u32 as i32);
const STG_E_READFAULT: HRESULT = HRESULT(0x8003_001e_u32 as i32);

#[implement(IStream)]
struct ReadSeekIStream {
    reader: Mutex<Box<dyn IncrementalReadSeek>>,
    length: u64,
}

impl ReadSeekIStream {
    fn open(
        mut reader: Box<dyn IncrementalReadSeek>,
        max_encoded_bytes: usize,
    ) -> windows::core::Result<IStream> {
        let length = reader
            .seek(SeekFrom::End(0))
            .map_err(|_| windows::core::Error::from_hresult(STG_E_SEEKERROR))?;
        if length == 0 || length > max_encoded_bytes as u64 {
            return Err(windows::core::Error::from_hresult(STG_E_INVALIDFUNCTION));
        }
        reader
            .seek(SeekFrom::Start(0))
            .map_err(|_| windows::core::Error::from_hresult(STG_E_SEEKERROR))?;
        Ok(ReadSeekIStream {
            reader: Mutex::new(reader),
            length,
        }
        .into())
    }
}

impl ISequentialStream_Impl for ReadSeekIStream_Impl {
    fn Read(&self, output: *mut core::ffi::c_void, count: u32, read: *mut u32) -> HRESULT {
        if count == 0 {
            if !read.is_null() {
                unsafe { read.write(0) };
            }
            return S_OK;
        }
        if output.is_null() && count != 0 {
            return STG_E_READFAULT;
        }
        let mut reader = match self.reader.lock() {
            Ok(reader) => reader,
            Err(_) => return STG_E_READFAULT,
        };
        let output = unsafe { std::slice::from_raw_parts_mut(output.cast::<u8>(), count as usize) };
        match reader.read(output) {
            Ok(actual) => {
                if !read.is_null() {
                    unsafe { read.write(actual as u32) };
                }
                if actual == count as usize {
                    S_OK
                } else {
                    S_FALSE
                }
            }
            Err(_) => STG_E_READFAULT,
        }
    }

    fn Write(&self, _input: *const core::ffi::c_void, _count: u32, written: *mut u32) -> HRESULT {
        if !written.is_null() {
            unsafe { written.write(0) };
        }
        STG_E_ACCESSDENIED
    }
}

impl IStream_Impl for ReadSeekIStream_Impl {
    fn Seek(
        &self,
        displacement: i64,
        origin: STREAM_SEEK,
        new_position: *mut u64,
    ) -> windows::core::Result<()> {
        let origin = if origin == STREAM_SEEK_SET {
            let offset = u64::try_from(displacement)
                .map_err(|_| windows::core::Error::from_hresult(STG_E_SEEKERROR))?;
            SeekFrom::Start(offset)
        } else if origin == STREAM_SEEK_CUR {
            SeekFrom::Current(displacement)
        } else if origin == STREAM_SEEK_END {
            SeekFrom::End(displacement)
        } else {
            return Err(windows::core::Error::from_hresult(STG_E_INVALIDFUNCTION));
        };
        let mut reader = self
            .reader
            .lock()
            .map_err(|_| windows::core::Error::from_hresult(STG_E_SEEKERROR))?;
        let previous = reader
            .stream_position()
            .map_err(|_| windows::core::Error::from_hresult(STG_E_SEEKERROR))?;
        let position = reader
            .seek(origin)
            .map_err(|_| windows::core::Error::from_hresult(STG_E_SEEKERROR))?;
        if position > self.length {
            let _ = reader.seek(SeekFrom::Start(previous));
            return Err(windows::core::Error::from_hresult(STG_E_SEEKERROR));
        }
        if !new_position.is_null() {
            unsafe { new_position.write(position) };
        }
        Ok(())
    }

    fn SetSize(&self, _new_size: u64) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(STG_E_ACCESSDENIED))
    }

    fn CopyTo(
        &self,
        _stream: windows::core::Ref<IStream>,
        _count: u64,
        read: *mut u64,
        written: *mut u64,
    ) -> windows::core::Result<()> {
        if !read.is_null() {
            unsafe { read.write(0) };
        }
        if !written.is_null() {
            unsafe { written.write(0) };
        }
        Err(windows::core::Error::from_hresult(STG_E_INVALIDFUNCTION))
    }

    fn Commit(&self, _flags: &STGC) -> windows::core::Result<()> {
        Ok(())
    }

    fn Revert(&self) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(STG_E_ACCESSDENIED))
    }

    fn LockRegion(
        &self,
        _offset: u64,
        _count: u64,
        _lock_type: &LOCKTYPE,
    ) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(STG_E_INVALIDFUNCTION))
    }

    fn UnlockRegion(
        &self,
        _offset: u64,
        _count: u64,
        _lock_type: u32,
    ) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(STG_E_INVALIDFUNCTION))
    }

    fn Stat(&self, stat: *mut STATSTG, _flags: &STATFLAG) -> windows::core::Result<()> {
        if stat.is_null() {
            return Err(windows::core::Error::from_hresult(STG_E_INVALIDFUNCTION));
        }
        let mut value: STATSTG = unsafe { std::mem::zeroed() };
        value.cbSize = self.length;
        value.r#type = 2;
        value.grfMode = STGM(0);
        unsafe { ptr::write(stat, value) };
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IStream> {
        Err(windows::core::Error::from_hresult(STG_E_INVALIDFUNCTION))
    }
}

pub(super) fn stream_from_reader(
    reader: Box<dyn IncrementalReadSeek>,
    max_encoded_bytes: usize,
) -> windows::core::Result<IStream> {
    if max_encoded_bytes == 0 {
        return Err(windows::core::Error::from_hresult(STG_E_INVALIDFUNCTION));
    }
    ReadSeekIStream::open(reader, max_encoded_bytes)
}
