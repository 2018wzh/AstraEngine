#![cfg(target_os = "android")]

use std::{
    fs::{File, OpenOptions},
    io::Write,
    mem::MaybeUninit,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use astra_media::{DecodeProvider, DecodedVideoFrame};

use astra_platform::{
    DecodeKind, DecodeOutput, DecodeStreamAction, PlatformDecodeRequest, PlatformError,
    PlatformErrorCode,
};

use ndk::media::{
    image_reader::{AcquireResult, Image, ImageFormat, ImageReader},
    media_codec::{AsyncNotifyCallback, BufferInfo, MediaCodec, MediaCodecDirection},
    media_format::MediaFormat,
};

mod decoder_contract;
use decoder_contract::decode_audio_media_codec;
use decoder_contract::decode_image;
use decoder_contract::decode_worker_loop;
pub(crate) use decoder_contract::prepare_scratch_root;
pub(crate) use decoder_contract::AndroidDecodeWorker;
use decoder_contract::AndroidVideoStream;
pub(crate) use decoder_contract::DecodeResource;
use decoder_contract::DecodeWork;
use decoder_contract::WorkerDecodeResource;
use decoder_contract::CALLBACK_IDLE_TIMEOUT;
use decoder_contract::CALLBACK_QUEUE_CAPACITY;
use decoder_contract::DECODE_TOTAL_TIMEOUT;
use decoder_contract::MAX_VIDEO_FRAMES;
mod android_video_stream;
use android_video_stream::codec_callback;
use android_video_stream::CodecEvent;
use android_video_stream::RawDecodedAudio;
mod drive_audio_codec;
use drive_audio_codec::decode_budget_error;
use drive_audio_codec::decode_error;
use drive_audio_codec::drive_audio_codec;
use drive_audio_codec::media_codec_error;
use drive_audio_codec::plane;
use drive_audio_codec::plane_value;
use drive_audio_codec::validate_codec_name;
use drive_audio_codec::yuv420_to_bgra;
use drive_audio_codec::Extractor;
use drive_audio_codec::Plane;
use drive_audio_codec::TempInput;
