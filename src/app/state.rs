use std::path::PathBuf;

/// State of the app-server streaming path for this process.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppMode {
    Off,
    Ready,
    Downgraded,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum V4Mode {
    Unknown,
    Available,
    Unavailable,
}

/// Borrow-free tag for a connection handshake phase.
#[derive(Clone, Copy)]
pub(crate) enum ConnectStage {
    ProviderRegistry,
    Create,
    Resume,
    Subscribe,
    V4Subscribe,
}

/// Availability of the kernel database used by optional live progress.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum DbState {
    Unknown,
    Enabled(PathBuf),
    Disabled,
}

/// Authoritative parent-session usage shown beside the composer.
///
/// Context occupancy comes from `state.updated`; cumulative token counters
/// come from `session/usage` (or the classic prompt summary fallback). Keeping
/// both in one snapshot prevents cumulative tokens from being mistaken for
/// the current context size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UsageSnapshot {
    pub(crate) context_used: Option<u64>,
    pub(crate) context_window: Option<u64>,
    pub(crate) total_tokens: Option<u64>,
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) reasoning_tokens: Option<u64>,
    pub(crate) cache_read_tokens: Option<u64>,
    pub(crate) model_request_count: Option<u64>,
}

impl UsageSnapshot {
    pub(crate) fn update_context(&mut self, used: u64, window: u64) {
        self.context_used = Some(used);
        self.context_window = Some(window);
    }

    pub(crate) fn update_context_used(&mut self, used: u64) {
        self.context_used = Some(used);
    }

    pub(crate) fn update_context_window(&mut self, window: u64) {
        if window > 0 {
            self.context_window = Some(window);
        }
    }

    pub(crate) fn update_session_usage(&mut self, result: &serde_json::Value) {
        fn number(result: &serde_json::Value, key: &str) -> Option<u64> {
            result.get(key).and_then(serde_json::Value::as_u64)
        }

        self.total_tokens = number(result, "totalTokens");
        self.input_tokens = number(result, "inputTokens");
        self.output_tokens = number(result, "outputTokens");
        self.reasoning_tokens = number(result, "reasoningTokens");
        self.cache_read_tokens = number(result, "cacheReadTokens");
        self.model_request_count = number(result, "modelRequestCount");
    }

    pub(crate) fn update_classic_summary(
        &mut self,
        context_used: Option<u64>,
        context_window: Option<u64>,
        total_tokens: Option<u64>,
    ) {
        if let (Some(used), Some(window)) = (context_used, context_window) {
            self.update_context(used, window);
        }
        if let Some(total) = total_tokens {
            self.total_tokens = Some(total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UsageSnapshot;

    #[test]
    fn usage_snapshot_keeps_context_and_cumulative_tokens_separate() {
        let mut usage = UsageSnapshot::default();
        usage.update_context(9_055, 200_000);
        usage.update_session_usage(&serde_json::json!({
            "totalTokens": 17_859,
            "inputTokens": 12_000,
            "outputTokens": 3_000,
            "reasoningTokens": 2_859,
            "cacheReadTokens": 8_000,
            "modelRequestCount": 4
        }));

        assert_eq!(usage.context_used, Some(9_055));
        assert_eq!(usage.context_window, Some(200_000));
        assert_eq!(usage.total_tokens, Some(17_859));
        assert_eq!(usage.input_tokens, Some(12_000));
        assert_eq!(usage.output_tokens, Some(3_000));
        assert_eq!(usage.reasoning_tokens, Some(2_859));
        assert_eq!(usage.cache_read_tokens, Some(8_000));
        assert_eq!(usage.model_request_count, Some(4));
    }
}
