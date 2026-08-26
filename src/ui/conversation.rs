use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::{live_panel_lines, UiState};

pub(crate) fn render_conversation(frame: &mut Frame<'_>, area: Rect, state: &mut UiState) {
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };
    let live_lines = live_panel_lines(state);
    if live_lines.is_empty() {
        return;
    }
    frame.render_widget(
        Paragraph::new(live_lines)
            .wrap(Wrap { trim: false })
            .scroll((0, 0)),
        inner,
    );
}
