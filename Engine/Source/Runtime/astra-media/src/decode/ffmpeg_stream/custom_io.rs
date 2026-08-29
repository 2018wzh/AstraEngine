use std::{
    ffi::c_void,
    io::{Read, Seek, SeekFrom},
    panic::{catch_unwind, AssertUnwindSafe},
    ptr, slice,
};

use ffmpeg_next as ffmpeg;

use super::{decode_error, MediaError};

const AVIO_BUFFER_BYTES: i32 = 64 * 1024;

trait ReadSeek: Read + Seek + Send {}

impl<T> ReadSeek for T where T: Read + Seek + Send {}

struct ReaderState {
    reader: Box<dyn ReadSeek>,
    length: u64,
}

/// Owns the callback state and buffer attached to an FFmpeg custom AVIO
/// context. The format input must be dropped before this owner.
pub(super) struct SeekableInputOwner {
    avio: *mut ffmpeg::ffi::AVIOContext,
    state: *mut ReaderState,
}

// FFmpeg and the playback decoder access this state exclusively from the
// decoder's owning worker. The callback reader itself is also Send.
unsafe impl Send for SeekableInputOwner {}

impl Drop for SeekableInputOwner {
    fn drop(&mut self) {
        unsafe {
            if !self.avio.is_null() {
                ffmpeg::ffi::av_freep(ptr::addr_of_mut!((*self.avio).buffer).cast());
                ffmpeg::ffi::avio_context_free(&mut self.avio);
            }
            if !self.state.is_null() {
                drop(Box::from_raw(self.state));
                self.state = ptr::null_mut();
            }
        }
    }
}

pub(super) fn open_seekable_input<R>(
    mut reader: R,
    max_encoded_bytes: usize,
) -> Result<(ffmpeg::format::context::Input, SeekableInputOwner), MediaError>
where
    R: Read + Seek + Send + 'static,
{
    ffmpeg::init().map_err(|error| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_PROBE",
            format!("initialize FFmpeg: {error}"),
        )
    })?;

    let length = reader.seek(SeekFrom::End(0)).map_err(|error| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_INPUT",
            format!("measure FFmpeg stream input: {error}"),
        )
    })?;
    let max_encoded_bytes = u64::try_from(max_encoded_bytes).map_err(|_| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_INPUT",
            "FFmpeg stream encoded byte budget exceeds the playback clock",
        )
    })?;
    if length == 0 || length > max_encoded_bytes {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_INPUT",
            "FFmpeg stream encoded byte budget is invalid",
        ));
    }
    reader.seek(SeekFrom::Start(0)).map_err(|error| {
        decode_error(
            "ASTRA_FFMPEG_STREAM_INPUT",
            format!("rewind FFmpeg stream input: {error}"),
        )
    })?;

    let state = Box::into_raw(Box::new(ReaderState {
        reader: Box::new(reader),
        length,
    }));
    let buffer = unsafe { ffmpeg::ffi::av_malloc(AVIO_BUFFER_BYTES as usize).cast::<u8>() };
    if buffer.is_null() {
        unsafe { drop(Box::from_raw(state)) };
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_IO",
            "allocate FFmpeg custom IO buffer",
        ));
    }

    let avio = unsafe {
        ffmpeg::ffi::avio_alloc_context(
            buffer,
            AVIO_BUFFER_BYTES,
            0,
            state.cast(),
            Some(read_packet),
            None,
            Some(seek),
        )
    };
    if avio.is_null() {
        unsafe {
            ffmpeg::ffi::av_free(buffer.cast());
            drop(Box::from_raw(state));
        }
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_IO",
            "create FFmpeg custom IO context",
        ));
    }
    unsafe {
        (*avio).seekable = ffmpeg::ffi::AVIO_SEEKABLE_NORMAL;
    }
    let owner = SeekableInputOwner { avio, state };

    let mut format = unsafe { ffmpeg::ffi::avformat_alloc_context() };
    if format.is_null() {
        return Err(decode_error(
            "ASTRA_FFMPEG_STREAM_DEMUX",
            "allocate FFmpeg format context",
        ));
    }
    unsafe {
        (*format).pb = avio;
        (*format).flags |= ffmpeg::ffi::AVFMT_FLAG_CUSTOM_IO;
    }

    let open_result = unsafe {
        ffmpeg::ffi::avformat_open_input(&mut format, ptr::null(), ptr::null_mut(), ptr::null_mut())
    };
    if open_result < 0 {
        close_format_context(&mut format);
        return Err(ffmpeg_result_error(
            "ASTRA_FFMPEG_STREAM_DEMUX",
            "open encoded stream",
            open_result,
        ));
    }

    let stream_result = unsafe { ffmpeg::ffi::avformat_find_stream_info(format, ptr::null_mut()) };
    if stream_result < 0 {
        close_format_context(&mut format);
        return Err(ffmpeg_result_error(
            "ASTRA_FFMPEG_STREAM_DEMUX",
            "inspect encoded stream",
            stream_result,
        ));
    }

    let input = unsafe { ffmpeg::format::context::Input::wrap(format) };
    Ok((input, owner))
}

fn close_format_context(format: &mut *mut ffmpeg::ffi::AVFormatContext) {
    if !format.is_null() {
        unsafe { ffmpeg::ffi::avformat_close_input(format) };
    }
}

fn ffmpeg_result_error(code: &'static str, operation: &'static str, result: i32) -> MediaError {
    decode_error(
        code,
        format!("{operation}: {}", ffmpeg::Error::from(result)),
    )
}

unsafe extern "C" fn read_packet(opaque: *mut c_void, buffer: *mut u8, buffer_size: i32) -> i32 {
    if opaque.is_null() || buffer.is_null() || buffer_size <= 0 {
        return i32::from(ffmpeg::Error::InvalidData);
    }
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let state = &mut *opaque.cast::<ReaderState>();
        let output = slice::from_raw_parts_mut(buffer, buffer_size as usize);
        state.reader.read(output)
    }));
    match result {
        Ok(Ok(0)) => i32::from(ffmpeg::Error::Eof),
        Ok(Ok(read)) => i32::try_from(read).unwrap_or(i32::from(ffmpeg::Error::Bug)),
        Ok(Err(_)) | Err(_) => i32::from(ffmpeg::Error::External),
    }
}

unsafe extern "C" fn seek(opaque: *mut c_void, offset: i64, whence: i32) -> i64 {
    if opaque.is_null() {
        return i64::from(i32::from(ffmpeg::Error::InvalidData));
    }
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let state = &mut *opaque.cast::<ReaderState>();
        let whence = whence & !ffmpeg::ffi::AVSEEK_FORCE;
        if whence == ffmpeg::ffi::AVSEEK_SIZE {
            return i64::try_from(state.length).map_err(|_| ());
        }
        let from = match whence {
            0 => SeekFrom::Start(u64::try_from(offset).map_err(|_| ())?),
            1 => SeekFrom::Current(offset),
            2 => SeekFrom::End(offset),
            _ => return Err(()),
        };
        let position = state.reader.seek(from).map_err(|_| ())?;
        if position > state.length {
            return Err(());
        }
        i64::try_from(position).map_err(|_| ())
    }));
    match result {
        Ok(Ok(position)) => position,
        Ok(Err(())) | Err(_) => i64::from(i32::from(ffmpeg::Error::External)),
    }
}
