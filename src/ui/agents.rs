use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::Frame;

use crate::UiState;

pub(crate) fn render_background_tasks(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let Some(selected) = state.agents.selected() else {
        return;
    };
    let t = &state.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(t.frame())
        .title(Line::from(Span::styled(
            " agents · observed lifecycle · ↑↓ selects · Esc closes ".to_string(),
            t.dim(),
        )));
    let inner = block.inner(area);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(inner);
    let items = state
        .agents
        .tasks()
        .iter()
        .map(|task| {
            let status = task.status.to_ascii_lowercase();
            let (symbol, style) = if status == "running" || status == "started" {
                ("●", t.accent())
            } else if status == "completed" || status == "success" {
                ("✓", t.good())
            } else if status.contains("fail") || status == "lost" {
                ("✗", t.bad())
            } else {
                ("·", t.dim())
            };
            let short_id: String = task.id.chars().take(18).collect();
            let pid = task.pid.map(|pid| format!("pid {pid}")).unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(format!("{symbol} {:<10}", task.status), style),
                Span::styled(format!("{:<14}", task.tool), t.text()),
                Span::styled(format!("{short_id:<20}"), t.dim()),
                Span::styled(pid, t.dim()),
            ]))
        })
        .collect::<Vec<_>>();
    let selected = selected.min(state.agents.tasks().len().saturating_sub(1));
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(t.selection())
            .highlight_symbol("› "),
        parts[0],
        &mut ListState::default().with_selected(Some(selected)),
    );

    if let Some(task) = state.agents.tasks().get(selected) {
        let command = task
            .command
            .as_deref()
            .unwrap_or("(not provided by kernel)");
        let command = command.replace(['\r', '\n'], " ");
        let command: String = command.chars().take(120).collect();
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("task: ".to_string(), t.dim()),
                    Span::styled(task.id.clone(), t.text()),
                ]),
                Line::from(vec![
                    Span::styled("command: ".to_string(), t.dim()),
                    Span::styled(command, t.text()),
                ]),
                Line::from(Span::styled(
                    "read-only: kernel exposes lifecycle events, not task logs or controls"
                        .to_string(),
                    t.dim(),
                )),
            ])
            .wrap(Wrap { trim: false }),
            parts[1],
        );
    }
}
