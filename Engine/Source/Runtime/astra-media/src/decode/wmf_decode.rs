use std::{ptr, slice};

use astra_core::Diagnostic;

use windows::{
    core::{Error as WindowsError, Interface, HRESULT},
    Win32::{
        Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK},
        Media::MediaFoundation::{
            IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MFAudioFormat_PCM,
            MFCreateAttributes, MFCreateMFByteStreamOnStreamEx, MFCreateMediaType,
            MFCreateSourceReaderFromByteStream, MFMediaType_Audio, MFMediaType_Video, MFShutdown,
            MFStartup, MFVideoFormat_RGB32, MFSTARTUP_FULL, MF_MT_AUDIO_NUM_CHANNELS,
            MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
            MF_MT_SUBTYPE, MF_PD_DURATION, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
            MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
            MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
            MF_SOURCE_READER_MEDIASOURCE, MF_VERSION,
        },
        System::{
            Com::{
                CoInitializeEx, CoUninitialize, StructuredStorage::CreateStreamOnHGlobal,
                COINIT_MULTITHREADED,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        },
    },
};

use super::{
    DecodeKind, DecodeOutput, DecodeRequest, DecodeResult, MediaError, WindowsDecodedAudioChunk,
    MAX_DECODED_AUDIO_BYTES, MAX_DECODED_VIDEO_FRAME_BYTES,
};

use crate::pcm_contract::{is_supported_pcm_channel_count, is_supported_pcm_rate};

mod startup;
pub(super) use startup::blocking;
pub(super) use startup::decode_audio;
pub(super) use startup::decode_error;
pub(super) use startup::decode_video;
pub(super) use startup::decode_video_stream;
pub(super) use startup::startup;
pub(super) use startup::IncrementalAudioDecoder;
pub(super) use startup::IncrementalVideoDecoder;
use startup::VideoOutput;
mod decode_video_inner;
use decode_video_inner::attribute_frame_duration_us;
use decode_video_inner::attribute_frame_size;
use decode_video_inner::audio_format_from_media_type;
use decode_video_inner::audio_stream_setup;
use decode_video_inner::decode_video_inner;
use decode_video_inner::decode_video_stream_inner;
use decode_video_inner::media_type;
use decode_video_inner::sample_bytes;
use decode_video_inner::sample_i16;
use decode_video_inner::source_reader_from_bytes;
use decode_video_inner::wmf_error;
