use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::{app::input::composer_layout, UiState};

pub(crate) fn render_composer(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    frame.render_widget(Paragraph::new("").style(t.band()), area);

    let mut lines: Vec<Line> = vec![Line::default()];
    if state.input.is_empty() {
        let placeholder = if area.width >= 54 {
            "describe a task   /commands   @files   !shell"
        } else if area.width >= 28 {
            "describe a task   /commands"
        } else {
            "describe a task"
        };
        lines.push(Line::from(vec![
            Span::styled(" › ".to_string(), t.accent().bold()),
            Span::styled(placeholder.to_string(), t.dim()),
        ]));
    } else {
        let content_width = area.width.saturating_sub(3).max(1) as usize;
        let layout = composer_layout(&state.input, state.cursor, content_width);
        let visible_rows = area.height.saturating_sub(2).max(1) as usize;
        let first_row = layout.cursor_row.saturating_sub(visible_rows - 1);
        for (offset, raw) in layout
            .lines
            .iter()
            .skip(first_row)
            .take(visible_rows)
            .enumerate()
        {
            let index = first_row + offset;
            let prefix = if index == 0 {
                Span::styled(" › ".to_string(), t.accent().bold())
            } else {
                Span::raw("   ".to_string())
            };
            lines.push(Line::from(vec![
                prefix,
                Span::styled(raw.clone(), t.text()),
            ]));
        }
        frame.render_widget(Paragraph::new(Text::from(lines)).style(t.band()), area);
        let cursor_x = area
            .x
            .saturating_add(3)
            .saturating_add(layout.cursor_col.min(content_width) as u16);
        let cursor_y = area.y.saturating_add(1).saturating_add(
            layout
                .cursor_row
                .saturating_sub(first_row)
                .min(visible_rows - 1) as u16,
        );
        frame.set_cursor_position((cursor_x, cursor_y));
        return;
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).style(t.band()), area);
    frame.set_cursor_position((area.x.saturating_add(3), area.y.saturating_add(1)));
}
