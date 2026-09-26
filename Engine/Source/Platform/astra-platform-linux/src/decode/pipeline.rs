use super::{cancelled_error, check_pipeline_error, host_error};
use astra_platform::PlatformError;
use gstreamer::{prelude::*, ClockTime};
use std::{
    io::Write,
    time::{Duration, Instant},
};

// These bound native request staging and delivered frame allocations; the product
// owns its cumulative playback budget. No complete decoded movie is retained.
pub(super) const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
pub(super) const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

pub(super) struct MediaPipeline {
    pub pipeline: gstreamer::Pipeline,
    sink: gstreamer_app::AppSink,
    bus: gstreamer::Bus,
    pub _input: tempfile::NamedTempFile,
    stopped: bool,
}

impl MediaPipeline {
    pub fn open(
        bytes: &[u8],
        conversion: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, PlatformError> {
        if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
            return Err(host_error(
                "decode.submit",
                "encoded input exceeds the Linux decode budget",
            ));
        }
        let mut input = tempfile::NamedTempFile::new().map_err(|_| {
            host_error(
                "decode.submit",
                "temporary media input could not be created",
            )
        })?;
        for chunk in bytes.chunks(64 * 1024) {
            if cancelled() {
                return Err(cancelled_error());
            }
            input.write_all(chunk).map_err(|_| {
                host_error(
                    "decode.submit",
                    "temporary media input could not be written",
                )
            })?;
        }
        let pipeline = gstreamer::parse::launch(&format!(
            "filesrc name=astra_input ! decodebin ! {conversion} ! appsink name=astra_sink sync=false max-buffers=2 drop=false wait-on-eos=false enable-last-sample=false"
        )).map_err(|_| host_error("decode.submit", "GStreamer pipeline could not be built"))?
            .downcast::<gstreamer::Pipeline>()
            .map_err(|_| host_error("decode.submit", "GStreamer pipeline has an invalid type"))?;
        let source = pipeline
            .by_name("astra_input")
            .ok_or_else(|| host_error("decode.submit", "GStreamer input is unavailable"))?;
        // A property value, never interpolated into the launch language.
        source.set_property(
            "location",
            input.path().to_str().ok_or_else(|| {
                host_error("decode.submit", "temporary media input path is unsupported")
            })?,
        );
        let sink = pipeline
            .by_name("astra_sink")
            .and_then(|element| element.downcast::<gstreamer_app::AppSink>().ok())
            .ok_or_else(|| host_error("decode.submit", "GStreamer appsink is unavailable"))?;
        let bus = pipeline
            .bus()
            .ok_or_else(|| host_error("decode.submit", "GStreamer bus is unavailable"))?;
        let result = Self {
            pipeline,
            sink,
            bus,
            _input: input,
            stopped: false,
        };
        result
            .pipeline
            .set_state(gstreamer::State::Playing)
            .map_err(|_| host_error("decode.submit", "GStreamer pipeline could not start"))?;
        Ok(result)
    }

    pub fn close(mut self) -> Result<(), PlatformError> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), PlatformError> {
        if !self.stopped {
            self.pipeline
                .set_state(gstreamer::State::Null)
                .map_err(|_| host_error("decode.close", "GStreamer pipeline could not stop"))?;
            self.stopped = true;
        }
        Ok(())
    }

    pub fn pull(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<gstreamer::Sample>, PlatformError> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if cancelled() {
                return Err(cancelled_error());
            }
            check_pipeline_error(&self.bus)?;
            let sample = self.sink.try_pull_sample(ClockTime::from_mseconds(20));
            check_pipeline_error(&self.bus)?;
            if cancelled() {
                return Err(cancelled_error());
            }
            if sample.is_some() {
                return Ok(sample);
            }
            if self.sink.is_eos() {
                return Ok(None);
            }
            if Instant::now() >= deadline {
                return Err(host_error("decode.submit", "GStreamer decode timed out"));
            }
        }
    }
}

impl Drop for MediaPipeline {
    fn drop(&mut self) {
        // NULL stops and joins GStreamer's streaming tasks before deleting input.
        if self.stop().is_err() {
            tracing::error!(
                event = "platform.linux.decode.shutdown_failed",
                diagnostic_code = "ASTRA_LINUX_GSTREAMER_SHUTDOWN_FAILED",
                "GStreamer pipeline could not stop"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[test]
    #[ignore = "requires GStreamer appsrc/appsink plugins"]
    fn stalled_native_pull_observes_cancellation_and_releases_pipeline() {
        gstreamer::init().unwrap();
        let pipeline = gstreamer::parse::launch(
            "appsrc is-live=true format=time ! appsink name=sink sync=false max-buffers=2 wait-on-eos=false"
        ).unwrap().downcast::<gstreamer::Pipeline>().unwrap();
        let sink = pipeline
            .by_name("sink")
            .unwrap()
            .downcast::<gstreamer_app::AppSink>()
            .unwrap();
        let media = MediaPipeline {
            bus: pipeline.bus().unwrap(),
            pipeline: pipeline.clone(),
            sink,
            _input: tempfile::NamedTempFile::new().unwrap(),
            stopped: false,
        };
        let path = media._input.path().to_path_buf();
        pipeline.set_state(gstreamer::State::Playing).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let trigger = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            flag.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let error = media
            .pull(&|| cancelled.load(Ordering::Acquire))
            .unwrap_err();
        assert_eq!(error.code, astra_platform::PlatformErrorCode::Cancelled);
        drop(media);
        trigger.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(pipeline.current_state(), gstreamer::State::Null);
        assert!(!path.exists());
    }
}
