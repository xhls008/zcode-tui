use super::{LogKind, LogLine};

pub(crate) fn log_entry_needs_separator(log: &[LogLine], index: usize) -> bool {
    index > 0 && log[index].kind != LogKind::User && log[index - 1].kind != LogKind::Assistant
}
