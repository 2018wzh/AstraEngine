use super::effects::validate_scene_filename;
use super::*;

#[cfg(test)]
mod tests;

impl MusicaVm {
    /// Persist only the movie cursor; decoder, PCM and GPU resources stay local.
    pub fn update_movie_position(
        &mut self,
        media_id: &str,
        fence_id: &str,
        position_us: u64,
    ) -> Result<(), MusicaRuntimeError> {
        if !matches!(&self.state.wait, Some(MusicaWaitState::Media { token_id, media_id: current })
            if token_id == fence_id && current == media_id)
        {
            return Err(MusicaRuntimeError::State);
        }
        let movie = self.state.movie.as_mut().ok_or(MusicaRuntimeError::State)?;
        if movie.media_id != media_id
            || movie.fence_id != fence_id
            || position_us < movie.continuation_pts
        {
            return Err(MusicaRuntimeError::State);
        }
        movie.continuation_pts = position_us;
        Ok(())
    }
}

pub(super) fn validate_movie_state(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    match (&state.movie, &state.wait) {
        (Some(movie), Some(MusicaWaitState::Media { token_id, media_id })) => {
            let id = movie
                .media_id
                .strip_prefix("musica.movie.")
                .and_then(|id| id.parse::<u32>().ok())
                .filter(|id| *id != 0)
                .ok_or(MusicaRuntimeError::State)?;
            if movie.media_id != format!("musica.movie.{id}")
                || media_id != &movie.media_id
                || token_id != &movie.fence_id
                || movie.fence_id != format!("musica.wait.movie.{}", state.instruction_count)
                || !(1..=8192).contains(&movie.width)
                || !(1..=8192).contains(&movie.height)
                || state.terminal
                || state.choice.is_some()
            {
                return Err(MusicaRuntimeError::State);
            }
            validate_scene_filename(
                movie
                    .resource_uri
                    .strip_prefix("musica:/mov/")
                    .ok_or(MusicaRuntimeError::State)?,
            )?;
            Ok(())
        }
        (None, Some(MusicaWaitState::Media { .. })) | (Some(_), _) => {
            Err(MusicaRuntimeError::State)
        }
        (None, _) => Ok(()),
    }
}

pub(super) fn execute_movie(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    let tokens = tokenize_operands(&command.raw_operands, command.span.offset as usize)
        .map_err(|_| MusicaRuntimeError::Operand)?;
    let [movie_id, resource, width, height, skippable] = tokens.as_slice() else {
        return Err(MusicaRuntimeError::Operand);
    };
    let movie_id = movie_id
        .parse::<u32>()
        .ok()
        .filter(|value| *value != 0)
        .ok_or(MusicaRuntimeError::Operand)?;
    validate_scene_filename(resource)?;
    let width = width
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=8192).contains(value))
        .ok_or(MusicaRuntimeError::Operand)?;
    let height = height
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=8192).contains(value))
        .ok_or(MusicaRuntimeError::Operand)?;
    let skippable = match skippable.as_str() {
        "t" => true,
        "f" => false,
        _ => return Err(MusicaRuntimeError::Operand),
    };
    if state.movie.is_some() {
        return Err(MusicaRuntimeError::State);
    }
    let media_id = format!("musica.movie.{movie_id}");
    let token_id = format!("musica.wait.movie.{}", state.instruction_count);
    let movie = MusicaMovieState {
        media_id: media_id.clone(),
        resource_uri: format!("musica:/mov/{resource}"),
        width,
        height,
        skippable,
        continuation_pts: 0,
        fence_id: token_id.clone(),
    };
    state.movie = Some(movie.clone());
    state.wait = Some(MusicaWaitState::Media { token_id, media_id });
    Ok(Some(MusicaVmEvent::Movie(movie)))
}
