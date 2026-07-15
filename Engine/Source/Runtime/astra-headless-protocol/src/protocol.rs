use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{validate_symbol, Diagnostic, InputMessage, ProtocolError, HEADLESS_PROTOCOL_SCHEMA};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub schema: String,
    pub session: String,
    pub sequence: u64,
    pub tick: u64,
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    Open {
        profile_path: String,
        package_path: Option<String>,
        checkpoint_config_path: Option<String>,
        artifact_root: String,
    },
    Opened {
        profile_hash: String,
        provider_identity_hash: String,
    },
    Input {
        input: InputMessage,
    },
    Observation {
        key: String,
        value_hash: String,
    },
    Artifact {
        relative_path: String,
        sha256: String,
    },
    Diagnostic {
        diagnostic: Diagnostic,
    },
    Shutdown,
    ShutdownComplete {
        run_report_path: String,
        run_report_hash: String,
    },
}

impl Envelope {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != HEADLESS_PROTOCOL_SCHEMA {
            return Err(ProtocolError::invalid(
                "protocol.schema",
                "unsupported protocol schema",
            ));
        }
        validate_symbol("protocol.session", &self.session)?;
        if self.sequence == 0 {
            return Err(ProtocolError::invalid(
                "protocol.sequence",
                "sequence must be non-zero",
            ));
        }
        if let Message::Input { input } = &self.message {
            input.validate()?;
            if input.session != self.session
                || input.sequence != self.sequence
                || input.tick != self.tick
            {
                return Err(ProtocolError::invalid(
                    "protocol.input",
                    "input identity does not match envelope",
                ));
            }
        }
        Ok(())
    }
}
