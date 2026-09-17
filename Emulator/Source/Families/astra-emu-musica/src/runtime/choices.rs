use super::*;

pub(super) fn execute_select(
    command: &ScCommand,
    state: &mut MusicaRuntimeState,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    options(command)?;
    if state.choice.is_some() || state.wait.is_some() {
        return Err(MusicaRuntimeError::State);
    }
    state.choice = Some(MusicaChoiceState {
        source: command.span,
        selected_index: Some(0),
    });
    state.message = None;
    state.wait = Some(MusicaWaitState::Input {
        token_id: format!("musica.choice.{}", state.instruction_count),
    });
    Ok(Some(MusicaVmEvent::Choice))
}

fn options(command: &ScCommand) -> Result<Vec<(String, String)>, MusicaRuntimeError> {
    let ScControlFlow::Choice { targets } = &command.control_flow else {
        return Err(MusicaRuntimeError::Operand);
    };
    let tokens = command.tokens().map_err(|_| MusicaRuntimeError::Operand)?;
    if command.opcode != "select"
        || !(1..=4).contains(&targets.len())
        || tokens.len() != targets.len()
    {
        return Err(MusicaRuntimeError::Operand);
    }
    tokens
        .iter()
        .zip(targets)
        .map(|(token, target)| {
            let (display, parsed) = token.split_once(':').ok_or(MusicaRuntimeError::Operand)?;
            if display.is_empty() || parsed != target {
                return Err(MusicaRuntimeError::Operand);
            }
            Ok((display.to_owned(), target.clone()))
        })
        .collect()
}

fn current_options(
    script: &ScScript,
    state: &MusicaRuntimeState,
) -> Result<Vec<(String, String)>, MusicaRuntimeError> {
    let choice = state.choice.as_ref().ok_or(MusicaRuntimeError::State)?;
    let command = script
        .lines
        .iter()
        .find_map(|line| match &line.kind {
            ScLineKind::Command { command } if command.span == choice.source => Some(command),
            _ => None,
        })
        .ok_or(MusicaRuntimeError::State)?;
    options(command)
}

pub(super) fn validate_choice(
    script: &ScScript,
    state: &MusicaRuntimeState,
) -> Result<(), MusicaRuntimeError> {
    if let Some(choice) = &state.choice {
        let choices = current_options(script, state)?;
        if choice
            .selected_index
            .is_none_or(|index| index as usize >= choices.len())
            || !matches!(state.wait, Some(MusicaWaitState::Input { .. }))
        {
            return Err(MusicaRuntimeError::State);
        }
    }
    Ok(())
}

impl MusicaVm {
    pub fn focus_choice(&mut self, index: u32) -> Result<bool, MusicaRuntimeError> {
        validate_choice(&self.script, &self.state)?;
        if index as usize >= current_options(&self.script, &self.state)?.len() {
            return Err(MusicaRuntimeError::State);
        }
        let choice = self
            .state
            .choice
            .as_mut()
            .ok_or(MusicaRuntimeError::State)?;
        let changed = choice.selected_index != Some(index);
        choice.selected_index = Some(index);
        Ok(changed)
    }
    pub fn choice_display(&self) -> Result<Option<(Vec<String>, u32)>, MusicaRuntimeError> {
        validate_choice(&self.script, &self.state)?;
        self.state
            .choice
            .as_ref()
            .map(|choice| {
                Ok((
                    current_options(&self.script, &self.state)?
                        .into_iter()
                        .map(|(text, _)| text)
                        .collect(),
                    choice.selected_index.unwrap(),
                ))
            })
            .transpose()
    }
    pub fn move_choice(&mut self, direction: i32) -> Result<(), MusicaRuntimeError> {
        validate_choice(&self.script, &self.state)?;
        let count = current_options(&self.script, &self.state)?.len() as i32;
        let choice = self
            .state
            .choice
            .as_mut()
            .ok_or(MusicaRuntimeError::State)?;
        let current = choice.selected_index.ok_or(MusicaRuntimeError::State)? as i32;
        choice.selected_index = Some((current + direction.signum()).rem_euclid(count) as u32);
        Ok(())
    }
    pub fn commit_choice(&mut self) -> Result<(), MusicaRuntimeError> {
        validate_choice(&self.script, &self.state)?;
        let options = current_options(&self.script, &self.state)?;
        let choice = self
            .state
            .choice
            .as_ref()
            .ok_or(MusicaRuntimeError::State)?;
        let target = &options[choice.selected_index.unwrap() as usize].1;
        let pc = *self.labels.get(target).ok_or(MusicaRuntimeError::Label)?;
        self.state.pc_line = pc;
        self.state.choice = None;
        self.state.wait = None;
        Ok(())
    }
}
