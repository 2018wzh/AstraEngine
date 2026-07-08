use astra_player_core::{
    PlayerAutomationReport, PlayerAutomationScript, PlayerAutomationValidator,
    PlayerInputTranscript, PlayerPlatform,
};

pub const WINDOWS_SENDINPUT_MOUSE: &str = "sendinput.mouse";
pub const WINDOWS_SENDINPUT_KEYBOARD: &str = "sendinput.keyboard";
pub const WEB_CDP_MOUSE: &str = "cdp.mouse";
pub const WEB_CDP_KEYBOARD: &str = "cdp.keyboard";

#[derive(Debug, Clone, Default)]
pub struct WindowsSendInputHost;

impl WindowsSendInputHost {
    pub fn build_report(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        PlayerAutomationValidator.validate(script, transcript)
    }

    pub fn supports(script: &PlayerAutomationScript) -> bool {
        script.platform == PlayerPlatform::Windows
    }
}

#[derive(Debug, Clone, Default)]
pub struct WebCdpInputHost;

impl WebCdpInputHost {
    pub fn build_report(
        &self,
        script: &PlayerAutomationScript,
        transcript: &PlayerInputTranscript,
    ) -> PlayerAutomationReport {
        PlayerAutomationValidator.validate(script, transcript)
    }

    pub fn supports(script: &PlayerAutomationScript) -> bool {
        script.platform == PlayerPlatform::Web
    }
}
