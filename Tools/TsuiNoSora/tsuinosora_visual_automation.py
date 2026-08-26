from __future__ import annotations

import hashlib
import io
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import zlib
from collections import deque
from pathlib import Path

import ctypes
import ctypes.wintypes

from tsuinosora_constants import *
from tsuinosora_visual_image_utils import _normalize_visual_capture_image, _visual_nonblank_bbox, _rgba_crop_bytes, _resize_rgba_bytes, _visual_capture_project_resolution, _visual_capture_project_scale_filter
from tsuinosora_diagnostics import _dedupe_diagnostics, _is_safe_symbol
from tsuinosora_projectorrays_convert_bitmap import _write_rgba_png
from tsuinosora_rendering import _non_negative_int, _safe_identifier, _safe_work_relative_path
from tsuinosora_visual_screenshot import _resolve_visual_capture_launch_command, _string_list, _visual_capture_launch_environment

__all__ = ['run_visual_capture_automation', '_visual_capture_automation_execution_report', '_WindowsSendInputVisualCaptureRunner', '_WindowsVisualCaptureApi']

def run_visual_capture_automation(work_root: Path | str, visual_capture: dict) -> dict:
    work_root = Path(work_root)
    automation = visual_capture.get("capture_automation") if isinstance(visual_capture, dict) else None
    if not isinstance(automation, dict):
        return _visual_capture_automation_execution_report(
            "blocked",
            [],
            [
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_CONFIG_INVALID",
                    "message": "capture automation config is missing",
                }
            ],
        )
    backend = str(automation.get("backend", ""))
    if backend != "windows_sendinput":
        return _visual_capture_automation_execution_report(
            "blocked",
            [{"event": "backend_rejected", "backend": _safe_identifier(backend)}],
            [
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_BACKEND_INVALID",
                    "backend": backend if _is_safe_symbol(backend) else "unknown",
                    "message": "capture automation backend must be windows_sendinput",
                }
            ],
        )
    if sys.platform != "win32":
        return _visual_capture_automation_execution_report(
            "blocked",
            [{"event": "backend_unavailable", "backend": "windows_sendinput"}],
            [
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_BACKEND_UNAVAILABLE",
                    "backend": "windows_sendinput",
                    "message": "windows_sendinput visual capture requires a Windows desktop session",
                }
            ],
        )
    return _WindowsSendInputVisualCaptureRunner(work_root, visual_capture).run()


def _visual_capture_automation_execution_report(
    status: str,
    transcript: list[dict],
    diagnostics: list[dict],
    captured_checkpoint_count: int = 0,
    screenshot_count: int = 0,
) -> dict:
    captures = [
        {
            "checkpoint_id": event["checkpoint_id"],
            "role": event["role"],
            "hash": event["hash"],
        }
        for event in transcript
        if event.get("event") == "capture"
        and _is_safe_symbol(str(event.get("checkpoint_id", "")))
        and event.get("role") in {"original", "demo"}
        and _is_sha256(str(event.get("hash", "")))
    ]
    return {
        "schema": "tsuinosora.visual_capture_automation_execution.v1",
        "status": status if status in {"pass", "blocked"} else "blocked",
        "captured_checkpoint_count": _non_negative_int(captured_checkpoint_count),
        "screenshot_count": _non_negative_int(screenshot_count),
        "transcript_hash": _sha256_bytes(
            json.dumps(transcript, sort_keys=True, separators=(",", ":"), default=str).encode("utf-8")
        ),
        "captures": captures,
        "diagnostics": _dedupe_diagnostics(diagnostics),
    }


class _WindowsSendInputVisualCaptureRunner:
    def __init__(self, work_root: Path, visual_capture: dict):
        self.work_root = work_root
        self.visual_capture = visual_capture if isinstance(visual_capture, dict) else {}
        self.automation = self.visual_capture.get("capture_automation", {})
        self.api = _WindowsVisualCaptureApi()
        self.output_resolution = _visual_capture_project_resolution(work_root)
        self.scale_filter = _visual_capture_project_scale_filter(work_root)
        self.sessions: dict[str, dict] = {}
        self.transcript: list[dict] = []
        self.diagnostics: list[dict] = []
        self.captured_checkpoints: set[str] = set()
        self.screenshot_count = 0

    def run(self) -> dict:
        try:
            self._launch_sessions()
            if not self.diagnostics:
                self._run_scripts()
        finally:
            self._terminate_sessions()
        return _visual_capture_automation_execution_report(
            "blocked" if self.diagnostics else "pass",
            self.transcript,
            self.diagnostics,
            len(self.captured_checkpoints),
            self.screenshot_count,
        )

    def _launch_sessions(self) -> None:
        sessions = self.automation.get("sessions", [])
        if not isinstance(sessions, list) or not sessions:
            self._diagnostic(
                "TSUI_VISUAL_CAPTURE_AUTOMATION_SESSIONS_MISSING",
                "capture automation requires sessions",
            )
            return
        for raw in sessions:
            if not isinstance(raw, dict):
                self._diagnostic(
                    "TSUI_VISUAL_CAPTURE_AUTOMATION_SESSION_INVALID",
                    "capture automation session must be an object",
                )
                continue
            role = str(raw.get("role", ""))
            if role not in {"original", "demo"}:
                self._diagnostic(
                    "TSUI_VISUAL_CAPTURE_AUTOMATION_SESSION_ROLE_INVALID",
                    "capture automation session role is invalid",
                    role=role if _is_safe_symbol(role) else "unknown",
                )
                continue
            launch = raw.get("launch", {})
            command = _string_list(launch.get("command") if isinstance(launch, dict) else None)
            if not command:
                self._diagnostic(
                    "TSUI_VISUAL_CAPTURE_AUTOMATION_LAUNCH_INVALID",
                    "capture automation launch command is missing",
                    role=role,
                )
                continue
            cwd = launch.get("working_directory") if isinstance(launch, dict) else None
            cwd_arg = str(cwd) if isinstance(cwd, str) and cwd else None
            try:
                process = subprocess.Popen(
                    _resolve_visual_capture_launch_command(command, cwd_arg),
                    cwd=cwd_arg,
                    env=_visual_capture_launch_environment(os.environ, launch),
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
            except (OSError, ValueError):
                self._diagnostic(
                    "TSUI_VISUAL_CAPTURE_AUTOMATION_LAUNCH_FAILED",
                    "capture automation could not launch a session",
                    role=role,
                )
                continue
            self.sessions[role] = {
                "role": role,
                "process": process,
                "window_match": raw.get("window_match", {}),
                "startup_timeout_ms": _non_negative_int(raw.get("startup_timeout_ms", 15000)) or 15000,
                "hwnd": 0,
            }
            self.transcript.append({"event": "launch", "role": role})
        for role in ("original", "demo"):
            if role not in self.sessions:
                self._diagnostic(
                    "TSUI_VISUAL_CAPTURE_AUTOMATION_SESSION_MISSING",
                    "capture automation requires both original and demo sessions",
                    role=role,
                )
        for session in list(self.sessions.values()):
            self._wait_for_window(session)

    def _wait_for_window(self, session: dict) -> None:
        import time

        timeout_ms = _non_negative_int(session.get("startup_timeout_ms", 15000)) or 15000
        deadline = time.monotonic() + timeout_ms / 1000.0
        process = session.get("process")
        pid = int(getattr(process, "pid", 0))
        while time.monotonic() < deadline:
            hwnd = self.api.find_window(session.get("window_match", {}), pid)
            if hwnd:
                session["hwnd"] = hwnd
                self.transcript.append({"event": "window_ready", "role": session["role"]})
                return
            if process is not None and process.poll() is not None:
                self._diagnostic(
                    "TSUI_VISUAL_CAPTURE_AUTOMATION_PROCESS_EXITED",
                    "capture automation session exited before its window was available",
                    role=session["role"],
                    exit_code=max(int(process.returncode or 0), 0),
                )
                return
            time.sleep(0.05)
        self._diagnostic(
            "TSUI_VISUAL_CAPTURE_AUTOMATION_WINDOW_MISSING",
            "capture automation could not find the requested window",
            role=session["role"],
        )

    def _run_scripts(self) -> None:
        scripts = self.automation.get("input_scripts", [])
        if not isinstance(scripts, list) or not scripts:
            self._diagnostic(
                "TSUI_VISUAL_CAPTURE_AUTOMATION_INPUT_SCRIPTS_MISSING",
                "capture automation requires checkpoint input scripts",
            )
            return
        checkpoints = self._checkpoints_by_id()
        for script in scripts:
            if not isinstance(script, dict):
                self._diagnostic(
                    "TSUI_VISUAL_CAPTURE_AUTOMATION_INPUT_SCRIPT_INVALID",
                    "capture automation input script must be an object",
                )
                continue
            checkpoint_id = str(script.get("checkpoint_id", ""))
            if checkpoint_id not in checkpoints:
                self._diagnostic(
                    "TSUI_VISUAL_CAPTURE_AUTOMATION_CHECKPOINT_INVALID",
                    "capture automation input script targets an unknown checkpoint",
                    checkpoint_id=checkpoint_id if _is_safe_symbol(checkpoint_id) else "unknown",
                )
                continue
            steps = script.get("steps", [])
            if not isinstance(steps, list) or not steps:
                self._diagnostic(
                    "TSUI_VISUAL_CAPTURE_AUTOMATION_STEPS_MISSING",
                    "capture automation input script requires steps",
                    checkpoint_id=checkpoint_id,
                )
                continue
            for step in steps:
                if not isinstance(step, dict):
                    self._diagnostic(
                        "TSUI_VISUAL_CAPTURE_AUTOMATION_STEP_INVALID",
                        "capture automation step must be an object",
                        checkpoint_id=checkpoint_id,
                    )
                    continue
                self._run_step(checkpoint_id, checkpoints[checkpoint_id], step)

    def _run_step(self, checkpoint_id: str, checkpoint: dict, step: dict) -> None:
        import time

        kind = str(step.get("kind", ""))
        if kind == "wait":
            duration_ms = min(_non_negative_int(step.get("duration_ms", 0)), 30000)
            time.sleep(duration_ms / 1000.0)
            self.transcript.append({"event": "wait", "checkpoint_id": checkpoint_id, "duration_ms": duration_ms})
            return
        if kind == "focus":
            for role in self._step_roles(step):
                self._focus_role(role, checkpoint_id)
            return
        if kind == "key":
            key = str(step.get("key", ""))
            for role in self._step_roles(step):
                if self._focus_role(role, checkpoint_id) and self.api.send_key(key):
                    self.transcript.append({"event": "key", "checkpoint_id": checkpoint_id, "role": role, "key": _safe_identifier(key)})
                else:
                    self._diagnostic(
                        "TSUI_VISUAL_CAPTURE_AUTOMATION_KEY_FAILED",
                        "capture automation could not send a keyboard input",
                        checkpoint_id=checkpoint_id,
                        role=role,
                        step_kind="key",
                    )
            return
        if kind == "mouse":
            for role in self._step_roles(step):
                if self._focus_role(role, checkpoint_id) and self._send_mouse(role, checkpoint_id, step):
                    button = str(step.get("button", "left"))
                    self.transcript.append(
                        {
                            "event": "mouse",
                            "checkpoint_id": checkpoint_id,
                            "role": role,
                            "button": _safe_identifier(button),
                        }
                    )
                else:
                    self._diagnostic(
                        "TSUI_VISUAL_CAPTURE_AUTOMATION_MOUSE_FAILED",
                        "capture automation could not send a mouse input",
                        checkpoint_id=checkpoint_id,
                        role=role,
                        step_kind="mouse",
                    )
            return
        if kind == "capture":
            for role in self._step_roles(step):
                self._capture_role(role, checkpoint_id, checkpoint)
            return
        self._diagnostic(
            "TSUI_VISUAL_CAPTURE_AUTOMATION_STEP_KIND_INVALID",
            "capture automation step kind is invalid",
            checkpoint_id=checkpoint_id,
            step_kind=kind if _is_safe_symbol(kind) else "unknown",
        )

    def _step_roles(self, step: dict) -> list[str]:
        role = step.get("role")
        if isinstance(role, str) and role in self.sessions:
            return [role]
        if isinstance(role, str) and role:
            return []
        return [role for role in ("original", "demo") if role in self.sessions]

    def _focus_role(self, role: str, checkpoint_id: str) -> bool:
        session = self.sessions.get(role)
        hwnd = int(session.get("hwnd", 0)) if session else 0
        if not hwnd:
            self._diagnostic(
                "TSUI_VISUAL_CAPTURE_AUTOMATION_WINDOW_MISSING",
                "capture automation session window is unavailable",
                checkpoint_id=checkpoint_id,
                role=role,
            )
            return False
        if not self.api.focus_window(hwnd):
            self._diagnostic(
                "TSUI_VISUAL_CAPTURE_AUTOMATION_FOCUS_FAILED",
                "capture automation could not focus the requested window",
                checkpoint_id=checkpoint_id,
                role=role,
            )
            return False
        self.transcript.append({"event": "focus", "checkpoint_id": checkpoint_id, "role": role})
        return True

    def _send_mouse(self, role: str, checkpoint_id: str, step: dict) -> bool:
        session = self.sessions.get(role)
        hwnd = int(session.get("hwnd", 0)) if session else 0
        if not hwnd:
            return False
        point = self.api.client_point(hwnd, _non_negative_int(step.get("x", 0)), _non_negative_int(step.get("y", 0)))
        if point is None:
            point = self.api.client_center(hwnd)
        if point is None:
            return False
        button = str(step.get("button", "left"))
        if button not in {"left", "right", "middle"}:
            self._diagnostic(
                "TSUI_VISUAL_CAPTURE_AUTOMATION_MOUSE_BUTTON_INVALID",
                "capture automation mouse button is invalid",
                checkpoint_id=checkpoint_id,
                role=role,
                step_kind="mouse",
            )
            return False
        return self.api.send_mouse_click(point[0], point[1], button)

    def _capture_role(self, role: str, checkpoint_id: str, checkpoint: dict) -> None:
        session = self.sessions.get(role)
        hwnd = int(session.get("hwnd", 0)) if session else 0
        rel = _safe_work_relative_path(checkpoint.get(f"{role}_screenshot", ""))
        if not hwnd or not rel:
            self._diagnostic(
                "TSUI_VISUAL_CAPTURE_AUTOMATION_CAPTURE_TARGET_INVALID",
                "capture automation screenshot target is invalid",
                checkpoint_id=checkpoint_id,
                role=role,
                step_kind="capture",
            )
            return
        image = self.api.capture_client_rgba(hwnd)
        if image is None:
            self._diagnostic(
                "TSUI_VISUAL_CAPTURE_AUTOMATION_CAPTURE_FAILED",
                "capture automation could not capture the requested window",
                checkpoint_id=checkpoint_id,
                role=role,
                step_kind="capture",
            )
            return
        image = _normalize_visual_capture_image(image, self.output_resolution, self.scale_filter)
        output_path = self.work_root / rel
        try:
            _write_rgba_png(output_path, image["width"], image["height"], image["rgba"])
        except (OSError, ValueError):
            self._diagnostic(
                "TSUI_VISUAL_CAPTURE_AUTOMATION_CAPTURE_WRITE_FAILED",
                "capture automation could not write a screenshot",
                checkpoint_id=checkpoint_id,
                role=role,
                step_kind="capture",
            )
            return
        self.screenshot_count += 1
        self.captured_checkpoints.add(checkpoint_id)
        self.transcript.append(
            {
                "event": "capture",
                "checkpoint_id": checkpoint_id,
                "role": role,
                "width": image["width"],
                "height": image["height"],
                "hash": _sha256(output_path),
            }
        )

    def _checkpoints_by_id(self) -> dict[str, dict]:
        checkpoints = {}
        raw = self.visual_capture.get("checkpoints", [])
        if not isinstance(raw, list):
            return checkpoints
        for checkpoint in raw:
            if not isinstance(checkpoint, dict):
                continue
            checkpoint_id = str(checkpoint.get("checkpoint_id", ""))
            if _is_safe_symbol(checkpoint_id):
                checkpoints[checkpoint_id] = checkpoint
        return checkpoints

    def _terminate_sessions(self) -> None:
        for session in self.sessions.values():
            process = session.get("process")
            if process is not None and process.poll() is None:
                try:
                    process.terminate()
                except OSError:
                    pass

    def _diagnostic(self, code: str, message: str, **fields) -> None:
        diagnostic = {"code": code, "message": message}
        for key, value in fields.items():
            if key in {"checkpoint_id", "route_id", "region_id", "role", "backend", "step_kind", "phase"}:
                if isinstance(value, str) and _is_safe_symbol(value):
                    diagnostic[key] = value
            elif key in {"exit_code", "count", "duration_ms"}:
                diagnostic[key] = _non_negative_int(value)
        self.diagnostics.append(diagnostic)


class _WindowsVisualCaptureApi:
    def __init__(self):
        import ctypes
        from ctypes import wintypes


        self.ctypes = ctypes
        self.wintypes = wintypes
        self.user32 = ctypes.WinDLL("user32", use_last_error=True)
        self.gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
        self.kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self.SW_RESTORE = 9
        self.INPUT_KEYBOARD = 1
        self.INPUT_MOUSE = 0
        self.KEYEVENTF_KEYUP = 0x0002
        self.MOUSEEVENTF_LEFTDOWN = 0x0002
        self.MOUSEEVENTF_LEFTUP = 0x0004
        self.MOUSEEVENTF_RIGHTDOWN = 0x0008
        self.MOUSEEVENTF_RIGHTUP = 0x0010
        self.MOUSEEVENTF_MIDDLEDOWN = 0x0020
        self.MOUSEEVENTF_MIDDLEUP = 0x0040
        self.PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
        self.DIB_RGB_COLORS = 0
        self.SRCCOPY = 0x00CC0020
        self.POINT = self._point_struct()
        self.RECT = self._rect_struct()
        self.BITMAPINFO = self._bitmap_info_struct()
        self.INPUT = self._input_struct()
        self._configure_api()

    def _configure_api(self) -> None:
        ctypes = self.ctypes
        wintypes = self.wintypes
        self.user32.EnumWindows.argtypes = [ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM), wintypes.LPARAM]
        self.user32.EnumWindows.restype = wintypes.BOOL
        self.user32.IsWindowVisible.argtypes = [wintypes.HWND]
        self.user32.IsWindowVisible.restype = wintypes.BOOL
        self.user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
        self.user32.GetWindowTextLengthW.restype = ctypes.c_int
        self.user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
        self.user32.GetWindowTextW.restype = ctypes.c_int
        self.user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
        self.user32.GetClassNameW.restype = ctypes.c_int
        self.user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
        self.user32.GetWindowThreadProcessId.restype = wintypes.DWORD
        self.user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
        self.user32.ShowWindow.restype = wintypes.BOOL
        self.user32.SetForegroundWindow.argtypes = [wintypes.HWND]
        self.user32.SetForegroundWindow.restype = wintypes.BOOL
        self.user32.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(self.RECT)]
        self.user32.GetClientRect.restype = wintypes.BOOL
        self.user32.ClientToScreen.argtypes = [wintypes.HWND, ctypes.POINTER(self.POINT)]
        self.user32.ClientToScreen.restype = wintypes.BOOL
        self.user32.GetDC.argtypes = [wintypes.HWND]
        self.user32.GetDC.restype = wintypes.HDC
        self.user32.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
        self.user32.ReleaseDC.restype = ctypes.c_int
        self.user32.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
        self.user32.SetCursorPos.restype = wintypes.BOOL
        self.user32.SendInput.argtypes = [wintypes.UINT, ctypes.POINTER(self.INPUT), ctypes.c_int]
        self.user32.SendInput.restype = wintypes.UINT
        self.kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel32.OpenProcess.restype = wintypes.HANDLE
        self.kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel32.CloseHandle.restype = wintypes.BOOL
        self.kernel32.QueryFullProcessImageNameW.argtypes = [
            wintypes.HANDLE,
            wintypes.DWORD,
            wintypes.LPWSTR,
            ctypes.POINTER(wintypes.DWORD),
        ]
        self.kernel32.QueryFullProcessImageNameW.restype = wintypes.BOOL
        self.gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]
        self.gdi32.CreateCompatibleDC.restype = wintypes.HDC
        self.gdi32.CreateCompatibleBitmap.argtypes = [wintypes.HDC, ctypes.c_int, ctypes.c_int]
        self.gdi32.CreateCompatibleBitmap.restype = wintypes.HBITMAP
        self.gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HGDIOBJ]
        self.gdi32.SelectObject.restype = wintypes.HGDIOBJ
        self.gdi32.BitBlt.argtypes = [
            wintypes.HDC,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            wintypes.HDC,
            ctypes.c_int,
            ctypes.c_int,
            wintypes.DWORD,
        ]
        self.gdi32.BitBlt.restype = wintypes.BOOL
        self.gdi32.GetDIBits.argtypes = [
            wintypes.HDC,
            wintypes.HBITMAP,
            wintypes.UINT,
            wintypes.UINT,
            wintypes.LPVOID,
            ctypes.POINTER(self.BITMAPINFO),
            wintypes.UINT,
        ]
        self.gdi32.GetDIBits.restype = ctypes.c_int
        self.gdi32.DeleteObject.argtypes = [wintypes.HGDIOBJ]
        self.gdi32.DeleteObject.restype = wintypes.BOOL
        self.gdi32.DeleteDC.argtypes = [wintypes.HDC]
        self.gdi32.DeleteDC.restype = wintypes.BOOL

    def _point_struct(self):
        class POINT(self.ctypes.Structure):
            _fields_ = [("x", self.ctypes.c_long), ("y", self.ctypes.c_long)]

        return POINT

    def _rect_struct(self):
        class RECT(self.ctypes.Structure):
            _fields_ = [
                ("left", self.ctypes.c_long),
                ("top", self.ctypes.c_long),
                ("right", self.ctypes.c_long),
                ("bottom", self.ctypes.c_long),
            ]

        return RECT

    def _bitmap_info_struct(self):
        ctypes = self.ctypes

        class BITMAPINFOHEADER(ctypes.Structure):
            _fields_ = [
                ("biSize", ctypes.c_uint32),
                ("biWidth", ctypes.c_int32),
                ("biHeight", ctypes.c_int32),
                ("biPlanes", ctypes.c_uint16),
                ("biBitCount", ctypes.c_uint16),
                ("biCompression", ctypes.c_uint32),
                ("biSizeImage", ctypes.c_uint32),
                ("biXPelsPerMeter", ctypes.c_int32),
                ("biYPelsPerMeter", ctypes.c_int32),
                ("biClrUsed", ctypes.c_uint32),
                ("biClrImportant", ctypes.c_uint32),
            ]

        class BITMAPINFO(ctypes.Structure):
            _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", ctypes.c_uint32 * 3)]

        return BITMAPINFO

    def _input_struct(self):
        ctypes = self.ctypes
        wintypes = self.wintypes

        class MOUSEINPUT(ctypes.Structure):
            _fields_ = [
                ("dx", wintypes.LONG),
                ("dy", wintypes.LONG),
                ("mouseData", wintypes.DWORD),
                ("dwFlags", wintypes.DWORD),
                ("time", wintypes.DWORD),
                ("dwExtraInfo", ctypes.c_size_t),
            ]

        class KEYBDINPUT(ctypes.Structure):
            _fields_ = [
                ("wVk", wintypes.WORD),
                ("wScan", wintypes.WORD),
                ("dwFlags", wintypes.DWORD),
                ("time", wintypes.DWORD),
                ("dwExtraInfo", ctypes.c_size_t),
            ]

        class INPUT_UNION(ctypes.Union):
            _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT)]

        class INPUT(ctypes.Structure):
            _fields_ = [("type", wintypes.DWORD), ("union", INPUT_UNION)]

        self.MOUSEINPUT = MOUSEINPUT
        self.KEYBDINPUT = KEYBDINPUT
        return INPUT

    def find_window(self, match: object, pid_hint: int = 0) -> int:
        ctypes = self.ctypes
        wintypes = self.wintypes
        match = match if isinstance(match, dict) else {}
        title_contains = str(match.get("title_contains", "")).lower()
        class_name = str(match.get("class_name", "")).lower()
        process_name = str(match.get("process_name", "")).lower()
        found = {"hwnd": 0}

        callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

        def callback(hwnd, _lparam):
            if found["hwnd"] or not self.user32.IsWindowVisible(hwnd):
                return True
            title = self._window_text(hwnd).lower()
            cls = self._window_class(hwnd).lower()
            pid = self._window_pid(hwnd)
            if title_contains and title_contains not in title:
                return True
            if class_name and class_name not in cls:
                return True
            if process_name:
                actual_process_name = self._process_name(pid).lower()
                if actual_process_name != process_name and (not pid_hint or pid != pid_hint):
                    return True
            elif pid_hint and pid != pid_hint:
                return True
            found["hwnd"] = int(hwnd)
            return False

        self.user32.EnumWindows(callback_type(callback), 0)
        return found["hwnd"]

    def _window_text(self, hwnd: int) -> str:
        length = self.user32.GetWindowTextLengthW(hwnd)
        buffer = self.ctypes.create_unicode_buffer(length + 1)
        self.user32.GetWindowTextW(hwnd, buffer, length + 1)
        return buffer.value

    def _window_class(self, hwnd: int) -> str:
        buffer = self.ctypes.create_unicode_buffer(256)
        self.user32.GetClassNameW(hwnd, buffer, 256)
        return buffer.value

    def _window_pid(self, hwnd: int) -> int:
        pid = self.wintypes.DWORD(0)
        self.user32.GetWindowThreadProcessId(hwnd, self.ctypes.byref(pid))
        return int(pid.value)

    def _process_name(self, pid: int) -> str:
        handle = self.kernel32.OpenProcess(self.PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
        if not handle:
            return ""
        try:
            size = self.wintypes.DWORD(32768)
            buffer = self.ctypes.create_unicode_buffer(size.value)
            if not self.kernel32.QueryFullProcessImageNameW(handle, 0, buffer, self.ctypes.byref(size)):
                return ""
            return Path(buffer.value).name
        finally:
            self.kernel32.CloseHandle(handle)

    def focus_window(self, hwnd: int) -> bool:
        self.user32.ShowWindow(hwnd, self.SW_RESTORE)
        return bool(self.user32.SetForegroundWindow(hwnd))

    def send_key(self, key: str) -> bool:
        vk = self._vk_for_key(key)
        if vk is None:
            return False
        return self._send_keyboard(vk, 0) and self._send_keyboard(vk, self.KEYEVENTF_KEYUP)

    def _send_keyboard(self, vk: int, flags: int) -> bool:
        inp = self.INPUT()
        inp.type = self.INPUT_KEYBOARD
        inp.union.ki = self.KEYBDINPUT(vk, 0, flags, 0, 0)
        sent = self.user32.SendInput(1, self.ctypes.byref(inp), self.ctypes.sizeof(self.INPUT))
        return sent == 1

    def send_mouse_click(self, x: int, y: int, button: str) -> bool:
        if not self.user32.SetCursorPos(x, y):
            return False
        down, up = {
            "left": (self.MOUSEEVENTF_LEFTDOWN, self.MOUSEEVENTF_LEFTUP),
            "right": (self.MOUSEEVENTF_RIGHTDOWN, self.MOUSEEVENTF_RIGHTUP),
            "middle": (self.MOUSEEVENTF_MIDDLEDOWN, self.MOUSEEVENTF_MIDDLEUP),
        }[button]
        return self._send_mouse(down) and self._send_mouse(up)

    def _send_mouse(self, flags: int) -> bool:
        inp = self.INPUT()
        inp.type = self.INPUT_MOUSE
        inp.union.mi = self.MOUSEINPUT(0, 0, 0, flags, 0, 0)
        sent = self.user32.SendInput(1, self.ctypes.byref(inp), self.ctypes.sizeof(self.INPUT))
        return sent == 1

    def client_point(self, hwnd: int, x: int, y: int) -> tuple[int, int] | None:
        point = self.POINT(x, y)
        if not self.user32.ClientToScreen(hwnd, self.ctypes.byref(point)):
            return None
        return int(point.x), int(point.y)

    def client_center(self, hwnd: int) -> tuple[int, int] | None:
        rect = self.RECT()
        if not self.user32.GetClientRect(hwnd, self.ctypes.byref(rect)):
            return None
        return self.client_point(hwnd, max((rect.right - rect.left) // 2, 0), max((rect.bottom - rect.top) // 2, 0))

    def capture_client_rgba(self, hwnd: int) -> dict | None:
        rect = self.RECT()
        if not self.user32.GetClientRect(hwnd, self.ctypes.byref(rect)):
            return None
        width = max(int(rect.right - rect.left), 0)
        height = max(int(rect.bottom - rect.top), 0)
        if width <= 0 or height <= 0:
            return None
        window_dc = self.user32.GetDC(hwnd)
        memory_dc = self.gdi32.CreateCompatibleDC(window_dc)
        bitmap = self.gdi32.CreateCompatibleBitmap(window_dc, width, height)
        old_object = self.gdi32.SelectObject(memory_dc, bitmap)
        try:
            if not self.gdi32.BitBlt(memory_dc, 0, 0, width, height, window_dc, 0, 0, self.SRCCOPY):
                return None
            bmi = self.BITMAPINFO()
            bmi.bmiHeader.biSize = self.ctypes.sizeof(bmi.bmiHeader)
            bmi.bmiHeader.biWidth = width
            bmi.bmiHeader.biHeight = -height
            bmi.bmiHeader.biPlanes = 1
            bmi.bmiHeader.biBitCount = 32
            bmi.bmiHeader.biCompression = 0
            buffer = self.ctypes.create_string_buffer(width * height * 4)
            if self.gdi32.GetDIBits(memory_dc, bitmap, 0, height, buffer, self.ctypes.byref(bmi), self.DIB_RGB_COLORS) == 0:
                return None
            rgba = bytearray()
            bgra = buffer.raw
            for offset in range(0, len(bgra), 4):
                b, g, r, _a = bgra[offset : offset + 4]
                rgba.extend((r, g, b, 255))
            return {"width": width, "height": height, "rgba": bytes(rgba)}
        finally:
            if old_object:
                self.gdi32.SelectObject(memory_dc, old_object)
            if bitmap:
                self.gdi32.DeleteObject(bitmap)
            if memory_dc:
                self.gdi32.DeleteDC(memory_dc)
            if window_dc:
                self.user32.ReleaseDC(hwnd, window_dc)

    def _vk_for_key(self, key: str) -> int | None:
        normalized = str(key).strip().lower()
        named = {
            "enter": 0x0D,
            "return": 0x0D,
            "space": 0x20,
            "escape": 0x1B,
            "esc": 0x1B,
            "tab": 0x09,
            "backspace": 0x08,
            "left": 0x25,
            "up": 0x26,
            "right": 0x27,
            "down": 0x28,
            "page_up": 0x21,
            "page_down": 0x22,
            "home": 0x24,
            "end": 0x23,
        }
        if normalized in named:
            return named[normalized]
        if len(normalized) == 1 and "a" <= normalized <= "z":
            return ord(normalized.upper())
        if len(normalized) == 1 and "0" <= normalized <= "9":
            return ord(normalized)
        if re.fullmatch(r"f([1-9]|1[0-2])", normalized):
            return 0x70 + int(normalized[1:]) - 1
        return None
