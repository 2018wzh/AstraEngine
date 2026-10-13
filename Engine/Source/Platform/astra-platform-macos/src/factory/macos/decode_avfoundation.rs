use super::*;

#[allow(deprecated)]
pub(super) fn decode_avfoundation(
    request: PlatformDecodeRequest,
) -> Result<DecodeOutput, PlatformError> {
    use std::io::Write;
    use std::ptr::NonNull;

    use objc2::runtime::AnyObject;
    use objc2_av_foundation::{
        AVAsset, AVAssetReader, AVAssetReaderTrackOutput, AVMediaTypeAudio, AVMediaTypeVideo,
        AVURLAsset,
    };
    use objc2_core_video::{
        CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
        CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress,
    };
    use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

    const MAX_DECODED_BYTES: usize = 512 * 1024 * 1024;
    let suffix = format!(".{}", request.codec);
    let mut input = tempfile::Builder::new()
        .suffix(&suffix)
        .tempfile()
        .map_err(|_| {
            host_error(
                "decode.submit",
                "temporary media input could not be created",
            )
        })?;
    input.write_all(&request.bytes).map_err(|_| {
        host_error(
            "decode.submit",
            "temporary media input could not be written",
        )
    })?;
    let path = input
        .path()
        .to_str()
        .ok_or_else(|| host_error("decode.submit", "temporary media input path is not UTF-8"))?;
    let path = NSString::from_str(path);
    let url = NSURL::fileURLWithPath(&path);

    unsafe {
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        let asset: &AVAsset = &asset;
        let media_type = match request.kind {
            DecodeKind::Audio => AVMediaTypeAudio,
            DecodeKind::Video => AVMediaTypeVideo,
            DecodeKind::Image => unreachable!(),
        }
        .ok_or_else(|| host_error("decode.submit", "AVFoundation media type is unavailable"))?;
        let track = asset
            .tracksWithMediaType(media_type)
            .firstObject()
            .ok_or_else(|| host_error("decode.submit", "AVFoundation found no matching track"))?;
        let settings = if request.kind == DecodeKind::Audio {
            let keys = [
                NSString::from_str("AVFormatIDKey"),
                NSString::from_str("AVLinearPCMIsFloatKey"),
                NSString::from_str("AVLinearPCMBitDepthKey"),
                NSString::from_str("AVLinearPCMIsBigEndianKey"),
                NSString::from_str("AVLinearPCMIsNonInterleavedKey"),
                NSString::from_str("AVSampleRateKey"),
                NSString::from_str("AVNumberOfChannelsKey"),
            ];
            let values = [
                NSNumber::numberWithUnsignedInt(u32::from_be_bytes(*b"lpcm")),
                NSNumber::numberWithBool(true),
                NSNumber::numberWithInt(32),
                NSNumber::numberWithBool(false),
                NSNumber::numberWithBool(false),
                NSNumber::numberWithInt(48_000),
                NSNumber::numberWithInt(2),
            ];
            NSDictionary::from_slices(
                &keys.iter().map(|value| &**value).collect::<Vec<_>>(),
                &values.iter().map(|value| &**value).collect::<Vec<_>>(),
            )
        } else {
            NSDictionary::from_slices(
                &[&*NSString::from_str("PixelFormatType")],
                &[&*NSNumber::numberWithUnsignedInt(u32::from_be_bytes(
                    *b"BGRA",
                ))],
            )
        };
        let settings = settings.cast_unchecked::<NSString, AnyObject>();
        let output = AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
            &track,
            Some(settings),
        );
        let reader = AVAssetReader::assetReaderWithAsset_error(asset)
            .map_err(|_| host_error("decode.submit", "AVAssetReader could not open input"))?;
        if !reader.canAddOutput(&output) {
            return Err(host_error(
                "decode.submit",
                "AVAssetReader rejected track output",
            ));
        }
        reader.addOutput(&output);
        if !reader.startReading() {
            return Err(host_error("decode.submit", "AVAssetReader could not start"));
        }
        let mut bytes = Vec::new();
        let mut audio_samples = Vec::<f32>::new();
        while let Some(sample) = output.copyNextSampleBuffer() {
            if request.kind == DecodeKind::Audio {
                let block = sample.data_buffer().ok_or_else(|| {
                    host_error("decode.submit", "AVFoundation audio sample has no data")
                })?;
                let length = block.data_length();
                if !length.is_multiple_of(std::mem::size_of::<f32>()) {
                    return Err(host_error(
                        "decode.submit",
                        "AVFoundation audio sample ends inside an f32 value",
                    ));
                }
                let sample_count = length / std::mem::size_of::<f32>();
                let start = audio_samples.len();
                audio_samples.resize(
                    start
                        .checked_add(sample_count)
                        .filter(|size| {
                            size.saturating_mul(std::mem::size_of::<f32>()) <= MAX_DECODED_BYTES
                        })
                        .ok_or_else(|| {
                            host_error("decode.submit", "decoded audio exceeds the byte limit")
                        })?,
                    0.0,
                );
                let destination = NonNull::new(audio_samples[start..].as_mut_ptr().cast()).unwrap();
                if block.copy_data_bytes(0, length, destination) != 0 {
                    return Err(host_error(
                        "decode.submit",
                        "AVFoundation audio copy failed",
                    ));
                }
            } else {
                let image = sample.image_buffer().ok_or_else(|| {
                    host_error("decode.submit", "AVFoundation video sample has no image")
                })?;
                let flags = CVPixelBufferLockFlags(1);
                if CVPixelBufferLockBaseAddress(&image, flags) != 0 {
                    return Err(host_error(
                        "decode.submit",
                        "video pixel buffer lock failed",
                    ));
                }
                let width = CVPixelBufferGetWidth(&image);
                let height = CVPixelBufferGetHeight(&image);
                let stride = CVPixelBufferGetBytesPerRow(&image);
                let base = CVPixelBufferGetBaseAddress(&image).cast::<u8>();
                if base.is_null() || stride < width.saturating_mul(4) {
                    let _ = CVPixelBufferUnlockBaseAddress(&image, flags);
                    return Err(host_error(
                        "decode.submit",
                        "video pixel buffer layout is invalid",
                    ));
                }
                let frame_bytes = width
                    .checked_mul(height)
                    .and_then(|value| value.checked_mul(4))
                    .ok_or_else(|| {
                        host_error("decode.submit", "decoded video dimensions overflow")
                    })?;
                if bytes
                    .len()
                    .checked_add(frame_bytes)
                    .is_none_or(|size| size > MAX_DECODED_BYTES)
                {
                    let _ = CVPixelBufferUnlockBaseAddress(&image, flags);
                    return Err(host_error(
                        "decode.submit",
                        "decoded video exceeds the byte limit",
                    ));
                }
                for row in 0..height {
                    let source = std::slice::from_raw_parts(base.add(row * stride), width * 4);
                    for pixel in source.as_chunks::<4>().0.iter() {
                        bytes.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                    }
                }
                let _ = CVPixelBufferUnlockBaseAddress(&image, flags);
            }
        }
        if bytes.is_empty() && audio_samples.is_empty() {
            return Err(host_error(
                "decode.submit",
                "AVFoundation produced no decoded samples",
            ));
        }
        if request.kind == DecodeKind::Audio {
            Ok(DecodeOutput::AudioPcmF32 {
                sample_rate: 48_000,
                channels: 2,
                samples: audio_samples,
            })
        } else {
            Ok(DecodeOutput::CpuBuffer {
                format: "rgba8".to_string(),
                bytes: bytes.into(),
            })
        }
    }
}

pub(super) fn media_decode_error(error: astra_media::MediaError) -> PlatformError {
    match error {
        astra_media::MediaError::Diagnostics(diagnostics) => {
            let diagnostic = diagnostics.into_iter().next();
            let mut error = PlatformError::new(
                PlatformErrorCode::ProviderUnavailable,
                "decode.submit",
                diagnostic
                    .as_ref()
                    .map_or("image decode failed", |value| value.message.as_str()),
            );
            if let Some(diagnostic) = diagnostic {
                error = error.with_field("diagnostic_code", diagnostic.code);
            }
            error
        }
        astra_media::MediaError::Message(message) => PlatformError::new(
            PlatformErrorCode::ProviderUnavailable,
            "decode.submit",
            message,
        ),
    }
}

pub(super) fn host_error(operation: &'static str, message: &'static str) -> PlatformError {
    PlatformError::new(PlatformErrorCode::ProviderUnavailable, operation, message)
}

pub(super) fn default_roots(package_id: &str) -> Option<super::super::HostRoots> {
    let project = directories::ProjectDirs::from("com", "AstraEngine", package_id)?;
    let executable = std::env::current_exe().ok()?;
    let contents = executable.parent()?.parent()?;
    Some(super::super::HostRoots {
        save_base: project.data_dir().join("SavedGames"),
        bundle_root: contents.join("Resources"),
    })
}
