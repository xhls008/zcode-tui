mod model;
mod presentation;

pub(crate) use model::{EntryId, LogKind, TranscriptEntry as LogLine};
pub(crate) use presentation::log_entry_needs_separator;
