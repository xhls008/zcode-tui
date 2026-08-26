use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

use crate::SUGGESTION_LIMIT;

pub(crate) struct ComposerLayout {
    pub(crate) lines: Vec<String>,
    pub(crate) cursor_row: usize,
    pub(crate) cursor_col: usize,
}

/// Wrap composer text by terminal cell width and locate the character cursor.
pub(crate) fn composer_layout(input: &str, cursor: usize, width: usize) -> ComposerLayout {
    let width = width.max(1);
    let chars = input.chars().collect::<Vec<_>>();
    let mut lines = vec![String::new()];
    let mut line_width = 0usize;
    for ch in &chars {
        if *ch == '\n' {
            lines.push(String::new());
            line_width = 0;
            continue;
        }
        let char_width = UnicodeWidthChar::width(*ch).unwrap_or(0);
        if line_width > 0 && line_width.saturating_add(char_width) > width {
            lines.push(String::new());
            line_width = 0;
        }
        lines.last_mut().expect("composer has a line").push(*ch);
        line_width = line_width.saturating_add(char_width);
    }

    let cursor = cursor.min(chars.len());
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    for ch in chars.iter().take(cursor) {
        if *ch == '\n' {
            cursor_row += 1;
            cursor_col = 0;
            continue;
        }
        let char_width = UnicodeWidthChar::width(*ch).unwrap_or(0);
        if cursor_col > 0 && cursor_col.saturating_add(char_width) > width {
            cursor_row += 1;
            cursor_col = 0;
        }
        cursor_col = cursor_col.saturating_add(char_width);
    }
    if cursor_col >= width {
        cursor_row += cursor_col / width;
        cursor_col %= width;
    } else if let Some(next) = chars.get(cursor).filter(|ch| **ch != '\n') {
        let next_width = UnicodeWidthChar::width(*next).unwrap_or(0);
        if cursor_col > 0 && cursor_col.saturating_add(next_width) > width {
            cursor_row += 1;
            cursor_col = 0;
        }
    }
    while lines.len() <= cursor_row {
        lines.push(String::new());
    }
    ComposerLayout {
        lines,
        cursor_row,
        cursor_col,
    }
}

pub(crate) fn suggestion_popup_area(
    viewport: Rect,
    input_area: Rect,
    item_count: usize,
) -> Option<Rect> {
    let requested_height = (item_count as u16).min(SUGGESTION_LIMIT as u16) + 2;
    let height = requested_height.min(input_area.y.saturating_sub(viewport.y));
    (height > 0).then_some(Rect {
        x: input_area.x,
        y: input_area.y - height,
        width: input_area.width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_wraps_ascii_cjk_and_explicit_lines() {
        let ascii = composer_layout("abcdef", 6, 5);
        assert_eq!(ascii.lines, vec!["abcde", "f"]);
        assert_eq!((ascii.cursor_row, ascii.cursor_col), (1, 1));

        let cjk = composer_layout("中文a", 3, 4);
        assert_eq!(cjk.lines, vec!["中文", "a"]);
        assert_eq!((cjk.cursor_row, cjk.cursor_col), (1, 1));

        let explicit = composer_layout("abcd\nef", 7, 4);
        assert_eq!(explicit.lines, vec!["abcd", "ef"]);
        assert_eq!((explicit.cursor_row, explicit.cursor_col), (1, 2));
    }
}
