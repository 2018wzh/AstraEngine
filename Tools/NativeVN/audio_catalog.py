"""Authoritative original-audio catalog for the NativeVN flagship sample."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class AudioSpec:
    asset_id: str
    kind: str
    duration_seconds: float
    seed: int
    title_en: str
    title_zh: str
    synthesis: str
    loop: bool = False


AUDIO_SPECS: tuple[AudioSpec, ...] = (
    AudioSpec("glass-rain", "bgm", 72.0, 1103, "Glass Rain", "玻璃雨", "rain_glass", True),
    AudioSpec("carrier-noise", "bgm", 84.0, 2207, "Carrier Noise", "载波噪声", "carrier", True),
    AudioSpec("thin-line", "bgm", 66.0, 3301, "Thin Line", "细线", "thin_line", True),
    AudioSpec("after-signal", "bgm", 78.0, 4409, "After Signal", "信号之后", "after_signal", True),
    AudioSpec("truth", "stinger", 7.2, 5101, "Truth", "真相", "truth"),
    AudioSpec("silence", "stinger", 6.4, 5209, "Silence", "寂静", "silence"),
    AudioSpec("signal", "stinger", 7.8, 5303, "Signal", "信号", "signal"),
    AudioSpec("rain-window", "se", 6.0, 6101, "Rain on Window", "雨落窗面", "rain_window"),
    AudioSpec("distant-thunder", "se", 5.2, 6203, "Distant Thunder", "远雷", "thunder"),
    AudioSpec("fluorescent-hum", "se", 4.0, 6301, "Fluorescent Hum", "荧光灯嗡鸣", "electrical_hum"),
    AudioSpec("radio-static", "se", 3.4, 6503, "Radio Static", "无线电静电", "radio_static"),
    AudioSpec("signal-lock", "se", 1.8, 6607, "Signal Lock", "信号锁定", "signal_lock"),
    AudioSpec("terminal-wake", "se", 2.3, 6701, "Terminal Wake", "终端唤醒", "terminal_wake"),
    AudioSpec("terminal-key", "se", 0.28, 6803, "Terminal Key", "终端按键", "terminal_key"),
    AudioSpec("terminal-confirm", "se", 0.65, 6907, "Terminal Confirm", "终端确认", "terminal_confirm"),
    AudioSpec("terminal-deny", "se", 0.82, 7001, "Terminal Deny", "终端拒绝", "terminal_deny"),
    AudioSpec("message-arrive", "se", 1.1, 7103, "Message Arrive", "消息抵达", "message_arrive"),
    AudioSpec("choice-open", "se", 0.75, 7207, "Choice Open", "选项展开", "choice_open"),
    AudioSpec("choice-select", "se", 0.48, 7307, "Choice Select", "选项确认", "choice_select"),
    AudioSpec("archive-open", "se", 1.45, 7403, "Archive Open", "档案开启", "archive_open"),
    AudioSpec("door-pressure", "se", 3.2, 7507, "Pressure Door", "气密门", "door_pressure"),
    AudioSpec("footstep-metal", "se", 0.72, 7603, "Metal Footstep", "金属脚步", "footstep_metal"),
    AudioSpec("glass-fracture", "se", 2.0, 7703, "Glass Fracture", "玻璃裂响", "glass_fracture"),
    AudioSpec("memory-flare", "se", 2.8, 7801, "Memory Flare", "记忆闪回", "memory_flare"),
    AudioSpec("scene-transition", "se", 1.6, 7907, "Scene Transition", "场景过渡", "scene_transition"),
)


def validate_catalog() -> None:
    ids = [item.asset_id for item in AUDIO_SPECS]
    if len(ids) != len(set(ids)):
        raise ValueError("audio catalog contains duplicate asset identifiers")
    counts = {kind: sum(item.kind == kind for item in AUDIO_SPECS) for kind in ("bgm", "stinger", "se")}
    if counts != {"bgm": 4, "stinger": 3, "se": 18}:
        raise ValueError(f"audio catalog shape is invalid: {counts}")
    for item in AUDIO_SPECS:
        lower, upper = {"bgm": (60.0, 90.0), "stinger": (5.0, 9.0), "se": (0.2, 8.0)}[item.kind]
        if not lower <= item.duration_seconds <= upper:
            raise ValueError(f"{item.asset_id} duration is outside the {item.kind} contract")


validate_catalog()
