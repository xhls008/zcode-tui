use ratatui::layout::Rect;
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::Frame;

pub(crate) fn render_conversation_items(
    frame: &mut Frame<'_>,
    area: Rect,
    items: Vec<ListItem<'static>>,
    anchor_bottom: bool,
) {
    if area.is_empty() || items.is_empty() {
        return;
    }
    let rows = items.iter().map(ListItem::height).sum::<usize>();
    let content_height = rows.min(usize::from(area.height)) as u16;
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: if anchor_bottom {
            area.bottom().saturating_sub(content_height)
        } else {
            area.y
        },
        width: area.width.saturating_sub(2),
        height: content_height,
    };
    let offset = rows.saturating_sub(usize::from(content_height));
    frame.render_stateful_widget(
        List::new(items),
        inner,
        &mut ListState::default().with_offset(offset),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::text::Line;
    use ratatui::Terminal;

    #[test]
    fn short_content_is_anchored_to_the_bottom() {
        let backend = TestBackend::new(20, 8);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_conversation_items(
                    frame,
                    frame.area(),
                    vec![
                        ListItem::new(Line::raw("question")),
                        ListItem::new(Line::raw("answer")),
                    ],
                    true,
                );
            })
            .expect("render conversation");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((1, 6)).expect("question cell").symbol(), "q");
        assert_eq!(buffer.cell((1, 7)).expect("answer cell").symbol(), "a");
    }

    #[test]
    fn active_content_starts_at_top_and_overflow_keeps_the_tail() {
        let backend = TestBackend::new(20, 4);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_conversation_items(
                    frame,
                    frame.area(),
                    vec![
                        ListItem::new(Line::raw("question")),
                        ListItem::new(Line::raw("thinking")),
                    ],
                    false,
                );
            })
            .expect("render active conversation");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((1, 0)).expect("question cell").symbol(), "q");
        assert_eq!(buffer.cell((1, 1)).expect("thinking cell").symbol(), "t");

        terminal
            .draw(|frame| {
                let items = (0..6)
                    .map(|row| ListItem::new(Line::raw(row.to_string())))
                    .collect();
                render_conversation_items(frame, frame.area(), items, false);
            })
            .expect("render overflowing conversation");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((1, 0)).expect("tail first cell").symbol(), "2");
        assert_eq!(buffer.cell((1, 3)).expect("tail last cell").symbol(), "5");
    }
}
