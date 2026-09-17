use astra_emu_sdk::video::*;

fn packet(generation: u64, sequence: u64, pts_us: u64, frames: u32) -> AudioFramePacket {
    AudioFramePacket {
        generation,
        sequence,
        resource_id: format!("audio.{sequence}"),
        pts_us,
        duration_us: u64::from(frames) * 125,
        sample_rate: 8000,
        channels: 2,
        frame_count: frames,
    }
}

#[test]
fn pcm_mixes_across_packets_preserves_existing_mix_and_stops_on_underrun() {
    let mut track = PcmQueue::new(8000, 2, 1, 0, 8, 4).unwrap();
    track
        .push(packet(1, 1, 0, 2), vec![16384, -16384, 8192, -8192])
        .unwrap();
    track
        .push(packet(1, 2, 500, 1), vec![32767, -32768])
        .unwrap();
    let mut mix = vec![0.125; 12];
    let result = track.mix_into(&mut mix).unwrap();
    assert_eq!(
        result,
        PcmMixResult {
            position_us: 625,
            advanced_frames: 5,
            starved: true
        }
    );
    assert_eq!(
        &mix[..8],
        &[0.625, -0.375, 0.375, -0.125, 0.125, 0.125, 0.125, 0.125]
    );
    assert_eq!(mix[8], 0.125 + 32767.0 / 32768.0);
    assert_eq!(mix[9], -0.875);
    assert_eq!(&mix[10..], &[0.125, 0.125]);
    assert!(track.is_empty());
    assert_eq!(track.resident_frames(), 0);
    assert_eq!(track.mix_into(&mut [0.0; 8]).unwrap().position_us, 625);
}

#[test]
fn pcm_seek_discards_old_generation_and_trims_decoder_preroll() {
    let mut track = PcmQueue::new(8000, 2, 1, 0, 8, 4).unwrap();
    track.push(packet(1, 1, 0, 4), vec![16384; 8]).unwrap();
    track.mix_into(&mut [0.0; 2]).unwrap();
    assert_eq!(track.resident_frames(), 4); // allocation, not unread suffix
    track.reset(2, 375).unwrap();
    assert_eq!(track.resident_frames(), 0);
    assert_eq!(
        track
            .push(packet(1, 2, 500, 1), vec![1; 2])
            .unwrap_err()
            .code(),
        "ASTRA_EMU_PCM_GENERATION"
    );
    track.push(packet(2, 1, 0, 2), vec![16384; 4]).unwrap();
    track.push(packet(2, 2, 250, 2), vec![8192; 4]).unwrap();
    let mut output = [0.0; 4];
    assert_eq!(track.mix_into(&mut output).unwrap().advanced_frames, 1);
    assert_eq!(output, [0.25, 0.25, 0.0, 0.0]);
    assert!(track.is_empty());
    assert!(track.reset(2, 0).is_err());
    assert_eq!(track.position_us(), 500);
}

#[test]
fn pcm_rejects_budget_format_overlap_and_sequence_without_mutating_queue() {
    let mut track = PcmQueue::new(8000, 2, 1, 0, 4, 1).unwrap();
    track.push(packet(1, 1, 0, 4), vec![0; 8]).unwrap();
    track.mix_into(&mut [0.0; 2]).unwrap();
    assert_eq!(
        track
            .push(packet(1, 2, 500, 1), vec![0; 2])
            .unwrap_err()
            .code(),
        "ASTRA_EMU_PCM_BUDGET"
    );
    assert_eq!(track.resident_frames(), 4);
    assert!(track.mix_into(&mut [0.0; 1]).is_err());
    assert_eq!(track.position_us(), 125);
    track.mix_into(&mut [0.0; 6]).unwrap();
    assert!(track.push(packet(1, 2, 0, 1), vec![0; 2]).is_err());
    assert!(track.push(packet(1, 1, 500, 1), vec![0; 2]).is_err());
    let mut wrong = packet(1, 2, 500, 1);
    wrong.sample_rate = 48000;
    assert!(track.push(wrong, vec![0; 2]).is_err());
    assert!(track.push(packet(1, 2, 500, 1), vec![0]).is_err());
    track.push(packet(1, 2, 500, 1), vec![8192; 2]).unwrap();
    let mut out = [0.0; 2];
    track.mix_into(&mut out).unwrap();
    assert_eq!(out, [0.25; 2]);
}
