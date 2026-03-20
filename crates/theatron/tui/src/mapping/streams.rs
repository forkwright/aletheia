//! Terminal, SSE, and stream event mapping methods.
/// Event-to-Msg translation: maps terminal, SSE, and stream events to application messages.
use crossterm::event::{Event as TermEvent, MouseButton, MouseEventKind};

use crate::api::types::SseEvent;
use crate::app::App;
use crate::events::{Event, StreamEvent};
use crate::msg::Msg;

impl App {
    pub fn map_event(&self, event: Event) -> Option<Msg> {
        match event {
            Event::Terminal(term_event) => self.map_terminal(term_event),
            Event::Sse(sse_event) => Some(self.map_sse(sse_event)),
            Event::Stream(stream_event) => Some(self.map_stream(stream_event)),
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
            SseEvent::Ping => Msg::Tick,
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
