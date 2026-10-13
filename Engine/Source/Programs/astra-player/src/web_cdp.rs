use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use base64::Engine;

use serde_json::{json, Value};

use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

use crate::{
    sha256_bytes, BundleContext, LiveInputRun, PlayerAutomationError,
    PlayerInputConsumptionEvidence, PlayerInputEvent, PlayerRuntimeRouteEvidence,
    PlayerVisualRegionEvidence, ScenarioInputAction, WEB_CDP_KEYBOARD,
};

pub use astra_player_core::WEB_PLAYER_LIVE_EVIDENCE_SCHEMA;

#[cfg(test)]
mod tests;

mod browser_contract;
pub use browser_contract::WebBrowserSession;
pub use browser_contract::WebBrowserSessionRequest;
pub use browser_contract::WebCdpRuntimeEvidence;
pub(crate) use browser_contract::WebLiveInputRun;
pub use browser_contract::WEB_PLAYER_EVIDENCE_PREFIX;
mod run_web_live_input;
pub(crate) use run_web_live_input::run_web_live_input;
pub use run_web_live_input::WebCdpSession;
mod web_cdp_session;
