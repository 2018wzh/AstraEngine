use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use astra_headless_protocol::{
    ButtonState, CheckpointConfig, Envelope, ImageTolerance, InputMessage, Message,
    ObservationPredicate, PhysicalInput, RunReport, RunStatus, HEADLESS_CHECKPOINT_CONFIG_SCHEMA,
    HEADLESS_PROTOCOL_SCHEMA, USER_INPUT_SEQUENCE_SCHEMA,
};

use astra_platform::HeadlessHostProfile;

use sha2::{Digest, Sha256};

#[path = "cli_contract/input_clock.rs"]
mod input_clock;
use input_clock::Fixture;
#[path = "cli_contract/review_identity.rs"]
mod review_identity;
use review_identity::command;
use review_identity::hash_messages;
use review_identity::input;
use review_identity::run_stdio;
use review_identity::write_jsonl;
