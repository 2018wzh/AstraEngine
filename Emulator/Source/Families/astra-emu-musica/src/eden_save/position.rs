use super::*;
use crate::{ScLineKind, ScScript, ScriptEncoding};
/// A checked native message boundary, not a reconstructed VM state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdenMessagePosition {
    pub message_line_index: u32,
    pub next_line_index: u32,
    pub message_id: i64,
}
impl EdenSave {
    /// Bind saved PC and message ID to the caller's actual decoded game script.
    /// No search for a nearby ID and no clamping of invalid positions is performed.
    pub fn validate_message_position(
        &self,
        filename: &str,
        script: &ScScript,
    ) -> Result<EdenMessagePosition, CoreError> {
        let invalid_position = || {
            invalid(
                "POSITION",
                "saved script identity or message boundary does not match",
            )
        };
        if self.variable("script_Filename") != Some(filename)
            || !matches!(
                (self.encoding, script.encoding),
                (EdenSaveEncoding::ShiftJis, ScriptEncoding::ShiftJis)
                    | (EdenSaveEncoding::Gbk, ScriptEncoding::Gbk)
            )
        {
            return Err(invalid_position());
        }
        let next_line_index = self
            .variable("script_Pointer")
            .ok_or_else(invalid_position)?
            .parse::<u32>()
            .map_err(|_| invalid_position())?;
        let message_line_index = next_line_index
            .checked_sub(1)
            .ok_or_else(invalid_position)?;
        let message_id = self
            .variable("script_ID")
            .ok_or_else(invalid_position)?
            .parse::<i64>()
            .map_err(|_| invalid_position())?;
        let line = script
            .lines
            .get(message_line_index as usize)
            .ok_or_else(invalid_position)?;
        let ScLineKind::Command { command } = &line.kind else {
            return Err(invalid_position());
        };
        if command.opcode != "message"
            || line
                .language_guard
                .is_some_and(|guard| guard != script.encoding.language())
        {
            return Err(invalid_position());
        }
        let tokens = command.tokens().map_err(|_| invalid_position())?;
        if tokens.len() < 4 || tokens[0].parse::<i64>().ok() != Some(message_id) {
            return Err(invalid_position());
        }
        Ok(EdenMessagePosition {
            message_line_index,
            next_line_index,
            message_id,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_native_pc_without_replay_clamping_or_id_search() {
        let mut save = super::super::tests::fixture();
        save.variables
            .iter_mut()
            .find(|v| v.name == "script_Pointer")
            .unwrap()
            .value = "2".into();
        save.variables.push(EdenSaveField {
            name: "script_ID".into(),
            value: "7".into(),
        });
        let script = crate::parse_sc_with_encoding(
            b"; fixture\n.message 7  speaker text\n.end\n",
            &crate::ScOpcodeCatalog::observed_musica(),
            ScriptEncoding::Gbk,
        )
        .unwrap();
        assert_eq!(
            save.validate_message_position("fixture.sc", &script)
                .unwrap()
                .message_line_index,
            1
        );
        assert!(save
            .validate_message_position("different.sc", &script)
            .is_err());
        for pc in ["0", "1", "3", "999999", "-1"] {
            save.variables
                .iter_mut()
                .find(|v| v.name == "script_Pointer")
                .unwrap()
                .value = pc.into();
            assert!(save
                .validate_message_position("fixture.sc", &script)
                .is_err());
        }
    }
}
