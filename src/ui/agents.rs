use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::Frame;

use crate::agents::{AgentWorkKind, InspectorTab, InspectorView};
use crate::UiState;

pub(crate) fn render_agent_inspector(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    if !state.agents.is_open() {
        return;
    }
    frame.render_widget(Clear, area);
    match state.agents.view() {
        InspectorView::List => render_list(frame, area, state),
        InspectorView::Detail => render_detail(frame, area, state),
    }
}

fn inspector_block<'a>(state: &UiState, title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(state.theme.frame())
        .title(Line::from(Span::styled(title, state.theme.dim())))
}

fn render_list(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let agents_tab = if state.agents.tab() == InspectorTab::Agents {
        "[ Agents ]"
    } else {
        "  Agents  "
    };
    let background_tab = if state.agents.tab() == InspectorTab::Background {
        "[ Background ]"
    } else {
        "  Background  "
    };
    let refresh = if state.agents.is_refreshing() {
        " · refreshing…"
    } else {
        ""
    };
    let title = format!(
        " inspector · {agents_tab} {background_tab}{refresh} · Tab switches · r refreshes "
    );
    let block = inspector_block(state, &title);
    let inner = block.inner(area);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);
    let mut items = Vec::new();
    if state.agents.tab() == InspectorTab::Agents {
        let status = if state.app_turn.is_some() || state.job.is_some() {
            "running"
        } else {
            "idle"
        };
        items.push(ListItem::new(status_line(
            state,
            status,
            "Parent Agent",
            "current conversation",
        )));
    }
    for task in state.agents.visible_tasks() {
        let label = task
            .title
            .as_deref()
            .filter(|title| !title.is_empty())
            .unwrap_or(&task.tool);
        let detail = task
            .output_tail
            .as_deref()
            .or(task.summary.as_deref())
            .or(task.command.as_deref())
            .unwrap_or(&task.id);
        items.push(ListItem::new(status_line(
            state,
            &task.status,
            label,
            detail,
        )));
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            if state.agents.is_refreshing() {
                "Refreshing official Agent state…"
            } else {
                "No work reported by the kernel. Press r to refresh."
            },
            t.dim(),
        ))));
    }
    frame.render_widget(block, area);
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(t.selection())
            .highlight_symbol("› "),
        parts[0],
        &mut ListState::default().with_selected(state.agents.selected()),
    );
    let cancel_hint = if state.agents.selected_cancel_eligible() {
        " · x cancels selected task"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "Enter details · ↑↓ select{cancel_hint} · Ctrl+X Y copies last reply · Esc closes"
                ),
                t.dim(),
            )),
            Line::from(vec![
                Span::styled("input target: ", t.dim()),
                Span::styled("parent", t.accent()),
                Span::styled(" · inspector is read-only", t.dim()),
            ]),
        ]),
        parts[1],
    );
}

fn status_line<'a>(state: &UiState, status: &str, label: &str, detail: &str) -> Line<'a> {
    let t = &state.theme;
    let lowered = status.to_ascii_lowercase();
    let (symbol, style) = if matches!(lowered.as_str(), "running" | "started") {
        ("●", t.accent())
    } else if matches!(lowered.as_str(), "completed" | "success" | "succeeded") {
        ("✓", t.good())
    } else if lowered.contains("fail") || matches!(lowered.as_str(), "lost" | "error") {
        ("✗", t.bad())
    } else {
        ("·", t.dim())
    };
    let detail: String = detail.replace(['\r', '\n'], " ").chars().take(72).collect();
    Line::from(vec![
        Span::styled(format!("{symbol} {status:<11}"), style),
        Span::styled(format!("{label:<20}"), t.text()),
        Span::styled(detail, t.dim()),
    ])
}

fn render_detail(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let title = " inspector detail · viewing: read-only · input target: parent ";
    let block = inspector_block(state, title);
    let inner = block.inner(area);
    let mut lines = vec![Line::from(vec![
        Span::styled("input target: ", t.dim()),
        Span::styled("parent", t.accent()),
        Span::styled(" · typed input never targets the viewed child", t.dim()),
    ])];
    lines.push(Line::default());

    if state.agents.selected_is_parent() {
        let status = if state.app_turn.is_some() || state.job.is_some() {
            "running"
        } else {
            "idle"
        };
        push_field(&mut lines, state, "type", "Parent Agent");
        push_field(&mut lines, state, "status", status);
        if let Some(session_id) = state.app_session.as_deref() {
            push_field(&mut lines, state, "sessionId", session_id);
        }
        push_field(
            &mut lines,
            state,
            "scope",
            "current conversation and composer target",
        );
    } else if let Some(task) = state.agents.selected_task() {
        push_field(
            &mut lines,
            state,
            "type",
            match task.kind {
                AgentWorkKind::Subagent => "Subagent",
                AgentWorkKind::Background => "Background Bash",
            },
        );
        if task.kind == AgentWorkKind::Subagent {
            push_field(
                &mut lines,
                state,
                "transcript",
                "full child transcript unavailable; live progress follows the parent V4 stream",
            );
        }
        push_field(&mut lines, state, "status", &task.status);
        push_optional(&mut lines, state, "title", task.title.as_deref());
        if let Some(output) = task.output_tail.as_deref() {
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::styled(
                    if task.kind == AgentWorkKind::Subagent {
                        "live progress"
                    } else {
                        "output tail"
                    },
                    t.accent(),
                ),
                Span::styled(" · newest 16k characters retained", t.dim()),
            ]));
            lines.extend(output.lines().map(|line| Line::from(line.to_string())));
        } else if task.kind == AgentWorkKind::Subagent && !status_is_terminal(&task.status) {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "live progress: waiting for the next official Subagent update…",
                t.dim(),
            )));
        }
        if task.summary.is_some() {
            lines.push(Line::default());
            push_optional(&mut lines, state, "final summary", task.summary.as_deref());
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled("identity and controls", t.dim())));
        push_optional(&mut lines, state, "taskId", task.task_id.as_deref());
        push_optional(
            &mut lines,
            state,
            "childSessionId",
            task.child_session_id.as_deref(),
        );
        push_optional(&mut lines, state, "agentId", task.agent_id.as_deref());
        push_optional(
            &mut lines,
            state,
            "toolCallId",
            task.tool_call_id.as_deref(),
        );
        if let Some(pid) = task.pid {
            push_field(&mut lines, state, "pid", &pid.to_string());
        }
        push_optional(&mut lines, state, "command", task.command.as_deref());
        push_field(
            &mut lines,
            state,
            "cancellable",
            if task.cancellable { "yes" } else { "no" },
        );
        let cancel_state = match task.task_id.as_deref() {
            Some(task_id) if state.agents.cancel_pending(task_id) => {
                "request pending; waiting for kernel response"
            }
            Some(_) if state.agents.selected_cancel_eligible() => "press x to cancel this task",
            _ => "unavailable for this record",
        };
        push_field(&mut lines, state, "cancel", cancel_state);
        if let Some(revision) = task.revision {
            push_field(&mut lines, state, "revision", &revision.to_string());
        }
        let linked = state.agents.linked_background(task);
        if !linked.is_empty() {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled("linked background work", t.dim())));
            for work in linked {
                lines.push(Line::from(vec![
                    Span::styled(format!("{}  ", work.status), t.text()),
                    Span::styled(
                        work.command.as_deref().unwrap_or(&work.id).to_string(),
                        t.dim(),
                    ),
                ]));
            }
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "↑↓/PgUp/PgDn scroll · r refreshes · Esc returns to list · terminal selection remains available",
        t.dim(),
    )));
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((state.agents.detail_scroll(), 0)),
        inner,
    );
}

fn status_is_terminal(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "completed"
            | "complete"
            | "success"
            | "succeeded"
            | "failed"
            | "error"
            | "lost"
            | "cancelled"
            | "canceled"
            | "stopped"
    )
}

fn push_optional<'a>(lines: &mut Vec<Line<'a>>, state: &UiState, label: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        push_field(lines, state, label, value);
    }
}

fn push_field<'a>(lines: &mut Vec<Line<'a>>, state: &UiState, label: &str, value: &str) {
    lines.push(Line::from(vec![
        Span::styled(format!("{label}: "), state.theme.dim()),
        Span::styled(value.to_string(), state.theme.text()),
    ]));
}
