use std::{
    fs,
    io::{BufReader, BufWriter},
    path::Path,
};

use astra_headless_protocol::{
    InputMessage, JsonlReader, JsonlWriter, PhysicalInput, SequenceValidator,
    USER_INPUT_SEQUENCE_SCHEMA,
};

pub struct ProductPerformanceInputRequest<'a> {
    pub prefix: &'a Path,
    pub output: &'a Path,
    pub warmup_frames: u64,
    pub measurement_frames: u64,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub point_a: (u32, u32),
    pub point_b: (u32, u32),
}

pub fn prepare(request: ProductPerformanceInputRequest<'_>) -> Result<(), String> {
    if request.warmup_frames == 0 || request.measurement_frames == 0 {
        return Err("ASTRA_PERFORMANCE_INPUT_FRAME_COUNT_INVALID".into());
    }
    let frame_count = request
        .warmup_frames
        .checked_add(request.measurement_frames)
        .ok_or("ASTRA_PERFORMANCE_INPUT_FRAME_COUNT_OVERFLOW")?;
    if frame_count > 1_000_000 {
        return Err("ASTRA_PERFORMANCE_INPUT_FRAME_COUNT_EXCEEDS_BOUND".into());
    }
    let point_a = normalized_point(
        request.point_a,
        request.viewport_width,
        request.viewport_height,
    )?;
    let point_b = normalized_point(
        request.point_b,
        request.viewport_width,
        request.viewport_height,
    )?;

    let file = fs::File::open(request.prefix)
        .map_err(|error| format!("ASTRA_PERFORMANCE_INPUT_PREFIX_OPEN_FAILED: {error}"))?;
    let mut reader =
        JsonlReader::new(BufReader::new(file), 1024 * 1024).map_err(|error| error.to_string())?;
    let mut prefix = Vec::<InputMessage>::new();
    let mut validator = SequenceValidator::default();
    let mut await_tick_shift = 0_u64;
    let mut last_effective_tick = 0_u64;
    while let Some(message) = reader
        .read::<InputMessage>()
        .map_err(|error| error.to_string())?
    {
        message.validate().map_err(|error| error.to_string())?;
        validator
            .accept(&message.session, message.sequence, message.tick)
            .map_err(|error| error.to_string())?;
        if matches!(message.event, PhysicalInput::Shutdown) {
            return Err("ASTRA_PERFORMANCE_INPUT_PREFIX_CONTAINS_SHUTDOWN".into());
        }
        last_effective_tick = message
            .tick
            .checked_sub(await_tick_shift)
            .ok_or("ASTRA_PERFORMANCE_INPUT_PREFIX_AWAIT_SHIFT_INVALID")?;
        if let PhysicalInput::Await {
            timeout_ticks,
            continue_at_match: true,
            ..
        } = &message.event
        {
            await_tick_shift = await_tick_shift
                .checked_add(u64::from(*timeout_ticks))
                .ok_or("ASTRA_PERFORMANCE_INPUT_PREFIX_AWAIT_SHIFT_OVERFLOW")?;
        }
        prefix.push(message);
    }
    let last = prefix
        .last()
        .ok_or("ASTRA_PERFORMANCE_INPUT_PREFIX_EMPTY")?;
    let session = last.session.clone();
    let start_sequence = last
        .sequence
        .checked_add(1)
        .ok_or("ASTRA_PERFORMANCE_INPUT_SEQUENCE_OVERFLOW")?;
    let start_tick = last_effective_tick
        .checked_add(1)
        .and_then(|tick| tick.checked_add(await_tick_shift))
        .ok_or("ASTRA_PERFORMANCE_INPUT_TICK_OVERFLOW")?;

    let parent = request.output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let name = request
        .output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("ASTRA_PERFORMANCE_INPUT_OUTPUT_INVALID")?;
    let temporary = request
        .output
        .with_file_name(format!(".{name}.partial-{}", std::process::id()));
    let output = fs::File::create(&temporary).map_err(|error| error.to_string())?;
    let mut writer = JsonlWriter::new(BufWriter::new(output));
    for message in &prefix {
        writer.write(message).map_err(|error| error.to_string())?;
    }
    for frame in 0..frame_count {
        let sequence = start_sequence
            .checked_add(frame)
            .ok_or("ASTRA_PERFORMANCE_INPUT_SEQUENCE_OVERFLOW")?;
        let tick = start_tick
            .checked_add(frame / 2)
            .ok_or("ASTRA_PERFORMANCE_INPUT_TICK_OVERFLOW")?;
        let (x, y) = if frame.is_multiple_of(2) {
            point_a
        } else {
            point_b
        };
        writer
            .write(&InputMessage {
                schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
                session: session.clone(),
                sequence,
                tick,
                event: PhysicalInput::PointerMove { x, y },
            })
            .map_err(|error| error.to_string())?;
    }
    writer
        .write(&InputMessage {
            schema: USER_INPUT_SEQUENCE_SCHEMA.into(),
            session,
            sequence: start_sequence
                .checked_add(frame_count)
                .ok_or("ASTRA_PERFORMANCE_INPUT_SEQUENCE_OVERFLOW")?,
            tick: start_tick
                .checked_add(frame_count.div_ceil(2))
                .ok_or("ASTRA_PERFORMANCE_INPUT_TICK_OVERFLOW")?,
            event: PhysicalInput::Shutdown,
        })
        .map_err(|error| error.to_string())?;
    drop(writer);
    fs::rename(&temporary, request.output).map_err(|error| error.to_string())?;
    println!(
        "{{\"schema\":\"astra.product_performance_input.v1\",\"start_sequence\":{start_sequence},\"warmup_frames\":{},\"measurement_frames\":{}}}",
        request.warmup_frames, request.measurement_frames
    );
    Ok(())
}

fn normalized_point(point: (u32, u32), width: u32, height: u32) -> Result<(u16, u16), String> {
    if width < 2 || height < 2 || point.0 >= width || point.1 >= height {
        return Err("ASTRA_PERFORMANCE_INPUT_POINTER_OUT_OF_BOUNDS".into());
    }
    let x = u64::from(point.0) * u64::from(u16::MAX) / u64::from(width - 1);
    let y = u64::from(point.1) * u64::from(u16::MAX) / u64::from(height - 1);
    Ok((x as u16, y as u16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_headless_protocol::{InputMessage, JsonlReader};

    #[test]
    fn generates_two_physical_frames_per_authoritative_tick() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("prefix.jsonl");
        fs::write(
            &prefix,
            "{\"schema\":\"astra.user_input_sequence.v1\",\"session\":\"demo.performance\",\"sequence\":1,\"tick\":0,\"event\":{\"type\":\"resume\"}}\n",
        )
        .unwrap();
        let output = temp.path().join("output.jsonl");
        prepare(ProductPerformanceInputRequest {
            prefix: &prefix,
            output: &output,
            warmup_frames: 2,
            measurement_frames: 2,
            viewport_width: 800,
            viewport_height: 600,
            point_a: (10, 20),
            point_b: (30, 40),
        })
        .unwrap();
        let file = fs::File::open(output).unwrap();
        let mut reader = JsonlReader::new(BufReader::new(file), 4096).unwrap();
        let mut messages = Vec::new();
        while let Some(message) = reader.read::<InputMessage>().unwrap() {
            messages.push(message);
        }
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[1].tick, 1);
        assert_eq!(messages[2].tick, 1);
        assert_eq!(messages[3].tick, 2);
        assert_eq!(messages[4].tick, 2);
        assert!(matches!(messages[5].event, PhysicalInput::Shutdown));
    }
}
