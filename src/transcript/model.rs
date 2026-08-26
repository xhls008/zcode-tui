use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ENTRY_ID: AtomicU64 = AtomicU64::new(1);

/// Stable identity for a transcript entry. It survives vector insertions and
/// removals and is never reused during the process lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EntryId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogKind {
    Banner,
    Logo,
    Tip,
    User,
    Assistant,
    System,
    Error,
    Diff,
    Tool,
}

#[derive(Debug)]
pub(crate) struct TranscriptEntry {
    pub(crate) id: EntryId,
    pub(crate) kind: LogKind,
    pub(crate) text: String,
}

impl TranscriptEntry {
    pub(crate) fn new(kind: LogKind, text: &str) -> Self {
        Self {
            id: EntryId(NEXT_ENTRY_ID.fetch_add(1, Ordering::Relaxed)),
            kind,
            text: text.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_identity_is_stable_across_vector_edits() {
        let first = TranscriptEntry::new(LogKind::User, "one");
        let first_id = first.id;
        let second = TranscriptEntry::new(LogKind::Assistant, "two");
        assert_ne!(first_id, second.id);

        let mut entries = vec![first, second];
        entries.insert(0, TranscriptEntry::new(LogKind::System, "zero"));
        assert_eq!(entries[1].id, first_id);
    }
}
