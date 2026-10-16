use super::*;
use crossbeam_channel::{select, RecvTimeoutError, TryRecvError};

pub(super) fn run_worker(
    rx: Receiver<Job>,
    sink: Arc<AudioSinkBox>,
    cancelled: Arc<AtomicBool>,
    playback_states: Arc<Mutex<BTreeMap<u32, bool>>>,
) -> FamilyResult<()> {
    // Keep one mixed block in flight. The mixer must still service commands
    // while the Host waits for PCM capacity, without advancing audio early.
    let (pcm_tx, pcm_rx) = bounded(1);
    let (completed_tx, completed_rx) = bounded(1);
    let output_sink = Arc::clone(&sink);
    let output = thread::Builder::new()
        .name("astra-fvp-pcm".into())
        .spawn(move || {
            while let Ok(samples) = pcm_rx.recv() {
                let result = write_pcm(&output_sink, samples);
                let failed = result.is_err();
                if completed_tx.send(result).is_err() || failed {
                    break;
                }
            }
        })
        .map_err(|_| {
            FamilyError::invalid("ASTRA_EMU_FVP_AUDIO_WORKER", "PCM worker could not start")
        })?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_mixer(
            rx,
            &sink,
            &cancelled,
            &playback_states,
            &pcm_tx,
            &completed_rx,
        )
    }));
    // Release a blocked write on every termination path, then join the sole
    // writer before the owning worker can finish.
    let cancel_result = sink.cancel().into_result();
    drop(pcm_tx);
    drop(completed_rx);
    let joined = output.join().map_err(|_| {
        FamilyError::invalid(
            "ASTRA_EMU_FVP_AUDIO_JOIN",
            "PCM worker panicked while stopping",
        )
    });
    match result {
        Ok(result) => result?,
        Err(payload) => std::panic::resume_unwind(payload),
    }
    cancel_result?;
    joined
}

fn run_mixer(
    rx: Receiver<Job>,
    sink: &AudioSinkBox,
    cancelled: &AtomicBool,
    playback_states: &Mutex<BTreeMap<u32, bool>>,
    pcm_tx: &Sender<Vec<i16>>,
    completed_rx: &Receiver<FamilyResult<()>>,
) -> FamilyResult<()> {
    let mut mixer = SoftAudioMixer::new(SymphoniaBackend, mixer_config());
    let mix_samples = mixer
        .config()
        .mix_frames
        .checked_mul(usize::from(OUTPUT.channels))
        .ok_or_else(|| {
            FamilyError::invalid("ASTRA_EMU_FVP_AUDIO_BUFFER", "audio buffer size overflow")
        })?;
    let result = (|| loop {
        if cancelled.load(Ordering::Acquire) || sink.is_cancelled() {
            return Ok(());
        }
        let mut received = false;
        loop {
            match rx.try_recv() {
                Ok(job) => {
                    received = true;
                    apply_job(&mut mixer, playback_states, job)?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
            if cancelled.load(Ordering::Acquire) || sink.is_cancelled() {
                return Ok(());
            }
        }
        let mut samples = vec![0_i16; mix_samples];
        let mixer_active = mixer
            .mix_next(&mut samples)
            .map_err(|cause| crate::error::rfvp_operation(cause, "audio mix"))?;
        refresh_playback_states(&mixer, playback_states);
        if mixer_active {
            pcm_tx.send(samples).map_err(|_| output_closed())?;
            loop {
                select! {
                    recv(completed_rx) -> completed => {
                        completed.map_err(|_| output_closed())??;
                        break;
                    }
                    recv(rx) -> command => {
                        let Ok(job) = command else { return Ok(()); };
                        apply_job(&mut mixer, playback_states, job)?;
                    }
                }
                if cancelled.load(Ordering::Acquire) || sink.is_cancelled() {
                    return Ok(());
                }
            }
        } else if !received {
            match rx.recv_timeout(COMMAND_WAIT) {
                Ok(job) => apply_job(&mut mixer, playback_states, job)?,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    })();
    mixer.shutdown();
    result
}

fn apply_job(
    mixer: &mut SoftAudioMixer<SymphoniaBackend>,
    playback_states: &Mutex<BTreeMap<u32, bool>>,
    job: Job,
) -> FamilyResult<()> {
    let result = apply_command(mixer, playback_states, job.command);
    let _ = job.completion.send(result.clone());
    result.map(|_| ())
}

fn output_closed() -> FamilyError {
    FamilyError::invalid("ASTRA_EMU_FVP_AUDIO_CLOSED", "PCM worker stopped")
}
