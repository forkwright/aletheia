# Tests for activity signal collector
from __future__ import annotations

import time
from datetime import datetime, timezone
from unittest.mock import patch

import pytest

from prosoche.signals import activity
from prosoche.signals.activity import (
    DEFAULT_DURATION_THRESHOLD,
    FocusEvent,
    _build_activity_summary,
    _record_focus_change,
    _reset_state,
    _sanitize_title,
    collect,
    get_active_window,
    poll_focus,
)


@pytest.fixture(autouse=True)
def clean_state():
    """Reset module state before each test."""
    _reset_state(ring_size=100)
    yield
    _reset_state(ring_size=100)


def _buf():
    """Get current ring buffer from module (avoids stale reference)."""
    return activity._ring_buffer


# ---------------------------------------------------------------------------
# Title sanitization
# ---------------------------------------------------------------------------

class TestSanitizeTitle:
    def test_strips_url_query_params(self):
        title = "GitHub - PRs?token=abc123&page=2 — Firefox"
        result = _sanitize_title(title)
        assert "abc123" not in result
        assert "?…" in result

    def test_truncates_long_titles(self):
        title = "A" * 300
        result = _sanitize_title(title)
        assert len(result) == 200
        assert result.endswith("...")

    def test_short_titles_unchanged(self):
        title = "aletheia/tui/src/app.rs - Neovim"
        assert _sanitize_title(title) == title


# ---------------------------------------------------------------------------
# Focus change recording
# ---------------------------------------------------------------------------

class TestFocusRecording:
    def test_first_focus_no_event_recorded(self):
        """First observation sets current focus but doesn't create an event."""
        _record_focus_change({"app_name": "Firefox", "window_title": "Test"}, time.monotonic())
        assert len(_buf()) == 0

    def test_focus_change_records_event(self):
        """Changing focus records the previous window's duration."""
        start = time.monotonic()
        _record_focus_change({"app_name": "Firefox", "window_title": "Tab 1"}, start)

        # Simulate time passing
        _record_focus_change(
            {"app_name": "Neovim", "window_title": "app.rs"},
            start + 30,  # 30 seconds later
        )

        assert len(_buf()) == 1
        event = _buf()[0]
        assert event.app_name == "Firefox"
        assert event.window_title == "Tab 1"
        assert event.duration_s == 30

    def test_short_focus_filtered(self):
        """Focus durations below threshold are not recorded."""
        start = time.monotonic()
        _record_focus_change({"app_name": "Firefox", "window_title": "Tab 1"}, start)
        _record_focus_change(
            {"app_name": "Neovim", "window_title": "app.rs"},
            start + 3,  # Only 3 seconds — below 10s threshold
        )

        assert len(_buf()) == 0

    def test_excluded_app_not_recorded(self):
        """Excluded apps are filtered out."""
        activity._excluded_apps = {"1password"}

        start = time.monotonic()
        _record_focus_change({"app_name": "1Password", "window_title": "Vault"}, start)
        _record_focus_change(
            {"app_name": "Firefox", "window_title": "Tab 1"},
            start + 60,
        )

        assert len(_buf()) == 0

    def test_ring_buffer_respects_max_size(self):
        """Ring buffer evicts oldest events when full."""
        _reset_state(ring_size=3)
        start = time.monotonic()
        for i in range(5):
            _record_focus_change(
                {"app_name": f"App{i}", "window_title": f"Win{i}"},
                start + (i * 20),
            )
        # After 5 focus changes, 4 events recorded but buffer holds 3
        assert len(_buf()) == 3
        assert _buf()[0].app_name == "App1"  # Oldest surviving


# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

class TestPlatformDetection:
    @patch("prosoche.signals.activity._get_active_window_gnome")
    def test_gnome_preferred(self, mock_gnome):
        mock_gnome.return_value = {"app_name": "Firefox", "window_title": "Test"}
        result = get_active_window()
        assert result["app_name"] == "Firefox"

    @patch("prosoche.signals.activity._get_active_window_gnome", return_value=None)
    @patch("prosoche.signals.activity._get_active_window_x11")
    def test_falls_through_to_x11(self, mock_x11, mock_gnome):
        mock_x11.return_value = {"app_name": "Alacritty", "window_title": "fish"}
        result = get_active_window()
        assert result["app_name"] == "Alacritty"

    @patch("prosoche.signals.activity._get_active_window_gnome", return_value=None)
    @patch("prosoche.signals.activity._get_active_window_x11", return_value=None)
    @patch("prosoche.signals.activity._get_active_window_macos", return_value=None)
    def test_returns_none_when_all_fail(self, *mocks):
        assert get_active_window() is None


# ---------------------------------------------------------------------------
# Activity summary
# ---------------------------------------------------------------------------

class TestActivitySummary:
    def test_empty_buffer_returns_empty(self):
        assert _build_activity_summary() == ""

    def test_summary_includes_recent_events(self):
        now = datetime.now(timezone.utc)
        _buf().append(FocusEvent(
            timestamp=now.isoformat(),
            app_name="Firefox",
            window_title="GitHub PR #300",
            duration_s=480,
        ))
        _buf().append(FocusEvent(
            timestamp=now.isoformat(),
            app_name="Neovim",
            window_title="aletheia/tui/src/app.rs",
            duration_s=2040,
        ))

        summary = _build_activity_summary(hours=2)
        assert "Firefox" in summary
        assert "Neovim" in summary
        assert "8 min" in summary
        assert "34 min" in summary

    def test_collapses_consecutive_same_app(self):
        now = datetime.now(timezone.utc)
        for i in range(3):
            _buf().append(FocusEvent(
                timestamp=now.isoformat(),
                app_name="Firefox",
                window_title=f"Tab {i}",
                duration_s=60,
            ))

        summary = _build_activity_summary(hours=2)
        # Should have only one Firefox line with combined duration
        firefox_lines = [l for l in summary.split("\n") if "Firefox" in l]
        assert len(firefox_lines) == 1
        assert "3 min" in firefox_lines[0]


# ---------------------------------------------------------------------------
# Signal collector
# ---------------------------------------------------------------------------

class TestCollector:
    @pytest.mark.anyio
    async def test_disabled_returns_empty(self):
        config = {"signals": {"activity": {"enabled": False}}}
        result = await collect(config)
        assert result == []

    @pytest.mark.anyio
    async def test_enabled_with_no_data_returns_empty(self):
        config = {"signals": {"activity": {"enabled": True}}}
        with patch("prosoche.signals.activity.get_active_window", return_value=None):
            result = await collect(config)
        assert result == []

    @pytest.mark.anyio
    async def test_emits_signal_with_activity(self):
        # Pre-populate ring buffer
        now = datetime.now(timezone.utc)
        _buf().append(FocusEvent(
            timestamp=now.isoformat(),
            app_name="Neovim",
            window_title="test.py",
            duration_s=600,
        ))

        config = {"signals": {"activity": {"enabled": True, "summary_hours": 2}}}
        with patch("prosoche.signals.activity.get_active_window", return_value=None):
            result = await collect(config)

        assert len(result) >= 1
        assert result[0].source == "activity"
        assert result[0].urgency == 0.1  # Informational
        assert result[0].context_blocks  # Has staged context

    @pytest.mark.anyio
    async def test_deep_focus_signal(self):
        """Long uninterrupted focus emits a deep work signal."""
        # Simulate being focused for 45 minutes
        now = time.monotonic()
        activity._current_focus = {
            "app_name": "Neovim",
            "window_title": "app.rs",
            "started_at": now - (45 * 60),  # 45 min ago
        }
        # Add something to ring buffer so summary isn't empty
        _buf().append(FocusEvent(
            timestamp=datetime.now(timezone.utc).isoformat(),
            app_name="Firefox",
            window_title="docs",
            duration_s=60,
        ))

        config = {"signals": {"activity": {"enabled": True, "deep_work_minutes": 30}}}
        with patch("prosoche.signals.activity.get_active_window", return_value={
            "app_name": "Neovim", "window_title": "app.rs"
        }):
            result = await collect(config)

        deep_signals = [s for s in result if "Deep focus" in s.summary]
        assert len(deep_signals) == 1
        assert deep_signals[0].urgency == 0.0  # Never interrupt deep work
