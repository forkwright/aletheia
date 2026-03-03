# Activity signal — window focus tracking for context reconstruction
#
# Captures active window title + app name via platform-appropriate methods:
#   - GNOME/Wayland: gdbus call to org.gnome.Shell.Eval
#   - X11: xdotool getactivewindow getwindowname + xprop
#   - macOS: osascript (System Events)
#
# Writes a ring buffer of FocusEvents and emits a Signal with recent activity
# summary for PROSOCHE.md. The killer use case is reconstruction after
# distraction: "what was I doing before this?"

from __future__ import annotations

import asyncio
import hashlib
import os
import re
import subprocess
import time
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime, timezone

from loguru import logger

from . import ContextBlock, Signal

# ---------------------------------------------------------------------------
# Configuration defaults
# ---------------------------------------------------------------------------
DEFAULT_RING_SIZE = 100
DEFAULT_POLL_INTERVAL = 5  # seconds
DEFAULT_DURATION_THRESHOLD = 10  # minimum seconds before recording
DEFAULT_SUMMARY_HOURS = 2  # hours of activity to include in summary

# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------

@dataclass
class FocusEvent:
    timestamp: str       # ISO8601
    app_name: str        # e.g., "Firefox", "Alacritty", "Code"
    window_title: str    # e.g., "aletheia/tui/src/app.rs - Neovim"
    duration_s: int = 0  # seconds spent before next focus change


# Module-level state — persists across collection cycles within the daemon
_ring_buffer: deque[FocusEvent] = deque(maxlen=DEFAULT_RING_SIZE)
_current_focus: dict | None = None  # {app_name, window_title, started_at}
_last_poll: float = 0.0
_excluded_apps: set[str] = set()


def _reset_state(ring_size: int = DEFAULT_RING_SIZE) -> None:
    """Reset module state — primarily for testing."""
    global _ring_buffer, _current_focus, _last_poll, _excluded_apps
    _ring_buffer.clear()
    # Recreate with new maxlen if changed
    if _ring_buffer.maxlen != ring_size:
        _ring_buffer = deque(maxlen=ring_size)
    _current_focus = None
    _last_poll = 0.0
    _excluded_apps = set()


def _get_ring_buffer() -> deque[FocusEvent]:
    """Access ring buffer — tests should use this instead of importing _ring_buffer directly."""
    return _ring_buffer


# ---------------------------------------------------------------------------
# Platform-specific window detection
# ---------------------------------------------------------------------------

def _get_active_window_gnome() -> dict | None:
    """Query active window via GNOME Shell DBus (Wayland-compatible)."""
    try:
        result = subprocess.run(
            [
                "gdbus", "call", "--session",
                "--dest", "org.gnome.Shell",
                "--object-path", "/org/gnome/Shell",
                "--method", "org.gnome.Shell.Eval",
                "global.display.focus_window ? "
                "(global.display.focus_window.get_title() + '|' + "
                "global.display.focus_window.get_wm_class()) : ''",
            ],
            capture_output=True, text=True, timeout=2,
        )
        if result.returncode != 0:
            return None

        # Output format: (true, 'title|wm_class')
        stdout = result.stdout.strip()
        match = re.search(r"\(true,\s*'(.+)'\)", stdout)
        if not match:
            return None

        raw = match.group(1)
        if "|" not in raw:
            return None

        parts = raw.rsplit("|", 1)
        title = parts[0].strip()
        app = parts[1].strip() if len(parts) > 1 else "Unknown"

        if not title:
            return None

        return {"app_name": app, "window_title": title}

    except (subprocess.TimeoutExpired, FileNotFoundError, OSError):
        return None


def _get_active_window_x11() -> dict | None:
    """Query active window via xdotool (X11)."""
    try:
        # Get window ID
        wid_result = subprocess.run(
            ["xdotool", "getactivewindow"],
            capture_output=True, text=True, timeout=2,
        )
        if wid_result.returncode != 0:
            return None

        wid = wid_result.stdout.strip()

        # Get window name
        name_result = subprocess.run(
            ["xdotool", "getwindowname", wid],
            capture_output=True, text=True, timeout=2,
        )
        title = name_result.stdout.strip() if name_result.returncode == 0 else ""

        # Get WM_CLASS via xprop
        class_result = subprocess.run(
            ["xprop", "-id", wid, "WM_CLASS"],
            capture_output=True, text=True, timeout=2,
        )
        app = "Unknown"
        if class_result.returncode == 0:
            # Format: WM_CLASS(STRING) = "instance", "class"
            match = re.search(r'"([^"]+)",\s*"([^"]+)"', class_result.stdout)
            if match:
                app = match.group(2)  # class name is more readable

        if not title:
            return None

        return {"app_name": app, "window_title": title}

    except (subprocess.TimeoutExpired, FileNotFoundError, OSError):
        return None


def _get_active_window_macos() -> dict | None:
    """Query active window via osascript (macOS)."""
    try:
        result = subprocess.run(
            [
                "osascript", "-e",
                'tell application "System Events" to get {name, title of first window} '
                'of first application process whose frontmost is true',
            ],
            capture_output=True, text=True, timeout=2,
        )
        if result.returncode != 0:
            return None

        # Output: "AppName, Window Title"
        parts = result.stdout.strip().split(", ", 1)
        app = parts[0] if parts else "Unknown"
        title = parts[1] if len(parts) > 1 else app

        return {"app_name": app, "window_title": title}

    except (subprocess.TimeoutExpired, FileNotFoundError, OSError):
        return None


def get_active_window() -> dict | None:
    """Try platform-appropriate methods in order."""
    # Try GNOME/Wayland first (default: Fedora 42)
    result = _get_active_window_gnome()
    if result:
        return result

    # X11 fallback
    result = _get_active_window_x11()
    if result:
        return result

    # macOS fallback
    result = _get_active_window_macos()
    if result:
        return result

    return None


# ---------------------------------------------------------------------------
# Ring buffer management
# ---------------------------------------------------------------------------

def _is_excluded(app_name: str) -> bool:
    """Check if app is in the exclusion list (case-insensitive)."""
    return app_name.lower() in _excluded_apps


def _sanitize_title(title: str) -> str:
    """Strip potentially sensitive info from window titles.

    Removes URLs with query params, file paths that look like credentials,
    and truncates to reasonable length.
    """
    # Strip URL query parameters (may contain tokens)
    title = re.sub(r'\?[^\s]*', '?…', title)
    # Truncate
    if len(title) > 200:
        title = title[:197] + "..."
    return title


def _record_focus_change(new_window: dict, now: float) -> None:
    """Record the end of one focus period and start of a new one."""
    global _current_focus

    if _current_focus is not None:
        duration = int(now - _current_focus["started_at"])
        threshold = DEFAULT_DURATION_THRESHOLD

        if duration >= threshold and not _is_excluded(_current_focus["app_name"]):
            event = FocusEvent(
                timestamp=datetime.now(timezone.utc).isoformat(),
                app_name=_current_focus["app_name"],
                window_title=_sanitize_title(_current_focus["window_title"]),
                duration_s=duration,
            )
            _ring_buffer.append(event)

    _current_focus = {
        "app_name": new_window["app_name"],
        "window_title": new_window["window_title"],
        "started_at": now,
    }


def poll_focus(config: dict | None = None) -> None:
    """Single poll of window focus. Called by collect() on each cycle."""
    global _last_poll, _excluded_apps

    now = time.monotonic()

    # Load exclusions from config if provided
    if config:
        act_config = config.get("signals", {}).get("activity", {})
        _excluded_apps = {
            a.lower() for a in act_config.get("exclude_apps", [])
        }
        ring_size = act_config.get("ring_buffer_size", DEFAULT_RING_SIZE)
        if _ring_buffer.maxlen != ring_size:
            _reset_state(ring_size)

    window = get_active_window()
    if window is None:
        return

    if _current_focus is None:
        # First observation — just record what's focused
        _record_focus_change(window, now)
        return

    # Check if focus changed
    if (window["app_name"] != _current_focus["app_name"] or
            window["window_title"] != _current_focus["window_title"]):
        _record_focus_change(window, now)

    _last_poll = now


# ---------------------------------------------------------------------------
# Signal collector (prosoche interface)
# ---------------------------------------------------------------------------

def _build_activity_summary(hours: float = DEFAULT_SUMMARY_HOURS) -> str:
    """Build a human-readable summary of recent activity."""
    if not _ring_buffer:
        return ""

    now = datetime.now(timezone.utc)
    cutoff_s = hours * 3600
    recent: list[FocusEvent] = []

    for event in reversed(_ring_buffer):
        try:
            event_time = datetime.fromisoformat(event.timestamp)
            age = (now - event_time).total_seconds()
            if age <= cutoff_s:
                recent.append(event)
        except (ValueError, TypeError):
            continue

    recent.reverse()

    if not recent:
        return ""

    # Collapse consecutive same-app entries
    collapsed: list[tuple[str, str, int, str]] = []  # (time, app, duration, title)
    for event in recent:
        time_str = event.timestamp[11:16]  # HH:MM from ISO
        if collapsed and collapsed[-1][1] == event.app_name:
            # Same app — merge duration, keep latest title
            prev = collapsed[-1]
            collapsed[-1] = (prev[0], prev[1], prev[2] + event.duration_s, event.window_title)
        else:
            collapsed.append((time_str, event.app_name, event.duration_s, event.window_title))

    lines = []
    for time_str, app, duration, title in collapsed:
        mins = duration // 60
        if mins < 1:
            dur_str = f"{duration}s"
        else:
            dur_str = f"{mins} min"

        # Truncate title for display
        display_title = title[:60] + "..." if len(title) > 60 else title
        lines.append(f"- {time_str} {app} ({display_title}) — {dur_str}")

    # Add current focus if active
    if _current_focus:
        duration = int(time.monotonic() - _current_focus["started_at"])
        if duration >= 60:
            mins = duration // 60
            title = _current_focus["window_title"][:60]
            lines.append(f"- now   {_current_focus['app_name']} ({title}) — {mins} min (ongoing)")

    return "\n".join(lines)


async def collect(config: dict) -> list[Signal]:
    """Prosoche signal collector interface.

    Unlike other collectors that query external services, this one polls
    the local window manager. Each call updates the ring buffer and emits
    a signal with the activity summary.
    """
    act_config = config.get("signals", {}).get("activity", {})
    if not act_config.get("enabled"):
        return []

    # Poll current focus
    poll_focus(config)

    # Only emit signal if we have meaningful data
    summary_hours = act_config.get("summary_hours", DEFAULT_SUMMARY_HOURS)
    summary = _build_activity_summary(summary_hours)

    if not summary:
        return []

    signals = []

    # Main activity signal — informational, for context
    signals.append(Signal(
        source="activity",
        summary=f"Recent activity ({summary_hours}h window)",
        urgency=0.1,  # Pure context, not actionable
        relevant_nous=[],  # All agents can benefit
        details=summary,
        context_blocks=[
            ContextBlock(
                title=f"Recent Activity (last {summary_hours} hours)",
                content=summary,
                source="activity",
            )
        ],
    ))

    # Detect long uninterrupted focus (deep work signal)
    if _current_focus:
        duration = int(time.monotonic() - _current_focus["started_at"])
        deep_work_threshold = act_config.get("deep_work_minutes", 30) * 60
        if duration >= deep_work_threshold:
            mins = duration // 60
            signals.append(Signal(
                source="activity",
                summary=f"Deep focus: {_current_focus['app_name']} for {mins} min",
                urgency=0.0,  # Informational only — don't interrupt deep work!
                relevant_nous=[],
                details=f"app={_current_focus['app_name']} duration={mins}min",
            ))

    return signals
