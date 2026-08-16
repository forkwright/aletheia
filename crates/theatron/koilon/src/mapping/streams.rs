//! Terminal, SSE, and stream event mapping methods.
/// Event-to-Msg translation: maps terminal, SSE, and stream events to application messages.
use crossterm::event::{Event as TermEvent, MouseButton, MouseEventKind};

use crate::api::types::SseEvent;
use crate::app::App;
use crate::events::{Event, StreamEvent};
use crate::msg::{Msg, NotificationKind};

impl App {
    pub(crate) fn map_event(&self, event: Event) -> Option<Msg> {
        match event {
            Event::Terminal(term_event) => self.map_terminal(term_event),
            Event::Sse(sse_event) => Some(self.map_sse(sse_event)),
            Event::Stream(stream_event) => Some(self.map_stream(stream_event)),
            Event::Background(msg) => Some(msg),
            Event::Tick => Some(Msg::Tick),
        }
    }

    fn map_terminal(&self, event: TermEvent) -> Option<Msg> {
        match event {
            TermEvent::Key(key) => self.map_key(key),
            TermEvent::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => Some(Msg::ScrollUp),
                MouseEventKind::ScrollDown => Some(Msg::ScrollDown),
                MouseEventKind::Down(MouseButton::Left) => {
                    let sidebar = crate::view::SIDEBAR_RECT.load_rect();
                    if sidebar.width > 0
                        && mouse.column < sidebar.x + sidebar.width
                        && mouse.row >= sidebar.y
                    {
                        let mut y = sidebar.y + 1;
                        for agent in &self.dashboard.agents {
                            let row_count = if agent.active_tool.is_some()
                                || agent.compaction_stage.is_some()
                            {
                                2u16
                            } else {
                                1
                            };
                            if mouse.row >= y && mouse.row < y + row_count {
                                return Some(Msg::FocusAgent(agent.id.clone()));
                            }
                            y += row_count;
                        }
                    }
                    None
                }
                _ => None,
            },
            TermEvent::Resize(w, h) => Some(Msg::Resize(w, h)),
            _ => None,
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "consistent method signature for event mapping interface"
    )]
    fn map_sse(&self, event: SseEvent) -> Msg {
        match event {
            SseEvent::Connected => Msg::SseConnected,
            SseEvent::Disconnected => Msg::SseDisconnected,
            SseEvent::Init { active_turns } => Msg::SseInit { active_turns },
            SseEvent::TurnBefore {
                nous_id,
                session_id,
                turn_id,
            } => Msg::SseTurnBefore {
                nous_id,
                session_id,
                turn_id,
            },
            SseEvent::TurnAfter {
                nous_id,
                session_id,
            }
            | SseEvent::TurnComplete {
                nous_id,
                session_id,
                ..
            } => Msg::SseTurnAfter {
                nous_id,
                session_id,
            },
            SseEvent::ToolCalled { nous_id, tool_name } => {
                Msg::SseToolCalled { nous_id, tool_name }
            }
            SseEvent::ToolFailed {
                nous_id,
                tool_name,
                error,
            } => Msg::SseToolFailed {
                nous_id,
                tool_name,
                error,
            },
            SseEvent::StatusUpdate { nous_id, status } => Msg::SseStatusUpdate { nous_id, status },
            SseEvent::SessionCreated {
                nous_id,
                session_id,
            } => Msg::SseSessionCreated {
                nous_id,
                session_id,
            },
            SseEvent::SessionArchived {
                nous_id,
                session_id,
            } => Msg::SseSessionArchived {
                nous_id,
                session_id,
            },
            SseEvent::DistillBefore { nous_id } => Msg::SseDistillBefore { nous_id },
            SseEvent::DistillStage { nous_id, stage } => Msg::SseDistillStage { nous_id, stage },
            SseEvent::DistillAfter { nous_id } => Msg::SseDistillAfter { nous_id },
            SseEvent::StreamLagged { dropped } => Msg::SseStreamLagged { dropped },
            SseEvent::Ping => Msg::Tick,
            // WHY(#6357): a server-emitted error is a persistent condition an
            // operator must acknowledge, not a transient one — route it to the
            // top-of-viewport banner instead of the auto-expiring local toast.
            SseEvent::Error { message } => Msg::ErrorBannerSet(message),
            // WHY(#6357): `nous.lifecycle` is a live EventBus topic (agent
            // created/restarted) this dashboard subscribes to but previously
            // dropped on the floor via the catch-all below. `restart_required`
            // is operator-actionable, so it renders as a Warning toast with a
            // longer dwell time than an informational lifecycle change.
            SseEvent::NousLifecycle {
                nous_id,
                event,
                restart_required,
            } => {
                let message = if restart_required {
                    format!("agent {nous_id}: {event} — restart required")
                } else {
                    format!("agent {nous_id}: {event}")
                };
                Msg::ToastPush {
                    message,
                    kind: if restart_required {
                        NotificationKind::Warning
                    } else {
                        NotificationKind::Info
                    },
                    duration_secs: if restart_required { 10 } else { 5 },
                }
            }
            // WHY(aletheia#4544): FactCreated/CheckpointCreated/CheckpointUpdated/
            // DecodeError/UnknownEvent previously fell through the catch-all
            // below and were silently dropped as `Msg::Tick` -- the exact
            // failure mode #6357 already fixed once for NousLifecycle. A
            // runtime contract test (`mapping::tests::every_sse_event_variant_is_handled`)
            // now pins that every variant maps to something other than
            // `Msg::Tick` (except the genuine no-op `Ping`), so this class of
            // drift fails a test instead of shipping silently again.
            SseEvent::FactCreated {
                fact_id: _,
                nous_id,
                content_preview,
            } => Msg::ToastPush {
                message: format!("fact recorded by {nous_id}: {content_preview}"),
                kind: NotificationKind::Info,
                duration_secs: 5,
            },
            SseEvent::CheckpointCreated {
                project_id,
                checkpoint_id,
            } => Msg::ToastPush {
                message: format!("checkpoint created for {project_id}: {checkpoint_id}"),
                kind: NotificationKind::Info,
                duration_secs: 5,
            },
            SseEvent::CheckpointUpdated {
                project_id,
                checkpoint_id,
                status,
            } => Msg::ToastPush {
                message: format!("checkpoint {checkpoint_id} ({project_id}): {status}"),
                kind: NotificationKind::Info,
                duration_secs: 5,
            },
            // WHY: a decode failure is a protocol-level problem, the same
            // persistent-condition class as `SseEvent::Error` above -- route
            // to the banner, not an auto-dismissing toast.
            SseEvent::DecodeError {
                event_type,
                raw_data: _,
                error,
            } => Msg::ErrorBannerSet(format!("SSE decode error on '{event_type}': {error}")),
            SseEvent::UnknownEvent {
                event_type,
                raw_data: _,
            } => Msg::ToastPush {
                message: format!(
                    "unrecognized SSE event '{event_type}' (client may be out of date)"
                ),
                kind: NotificationKind::Info,
                duration_secs: 5,
            },
            _ => Msg::Tick,
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "consistent method signature for event mapping interface"
    )]
    fn map_stream(&self, event: StreamEvent) -> Msg {
        match event {
            StreamEvent::TurnStart {
                session_id,
                nous_id,
                turn_id,
                request_id: _,
            } => Msg::StreamTurnStart {
                session_id,
                nous_id,
                turn_id,
            },
            StreamEvent::TextDelta(text) => Msg::StreamTextDelta(text),
            StreamEvent::ThinkingDelta(text) => Msg::StreamThinkingDelta(text),
            StreamEvent::ToolStart {
                tool_name,
                tool_id,
                input,
            } => Msg::StreamToolStart {
                tool_name,
                tool_id,
                input,
            },
            StreamEvent::ToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
                result,
            } => Msg::StreamToolResult {
                tool_name,
                tool_id,
                is_error,
                duration_ms,
                result,
            },
            StreamEvent::ToolApprovalRequired {
                turn_id,
                tool_name,
                tool_id,
                input,
                risk,
                reason,
            } => Msg::StreamToolApprovalRequired {
                turn_id,
                tool_name,
                tool_id,
                input,
                risk,
                reason,
            },
            StreamEvent::ToolApprovalResolved { tool_id, decision } => {
                Msg::StreamToolApprovalResolved { tool_id, decision }
            }
            StreamEvent::PlanProposed { plan } => Msg::StreamPlanProposed { plan },
            StreamEvent::PlanStepStart { plan_id, step_id } => {
                Msg::StreamPlanStepStart { plan_id, step_id }
            }
            StreamEvent::PlanStepComplete {
                plan_id,
                step_id,
                status,
            } => Msg::StreamPlanStepComplete {
                plan_id,
                step_id,
                status,
            },
            StreamEvent::PlanComplete { plan_id, status } => {
                Msg::StreamPlanComplete { plan_id, status }
            }
            StreamEvent::TurnComplete { outcome } => Msg::StreamTurnComplete { outcome },
            StreamEvent::TurnAbort { reason } => Msg::StreamTurnAbort { reason },
            StreamEvent::Error(msg) => Msg::StreamError(msg),
            _ => Msg::Tick,
        }
    }
}
