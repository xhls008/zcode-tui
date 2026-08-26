use zcode_tui::{AgentSnapshot, AppServerEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentWorkKind {
    Subagent,
    Background,
}

/// One child-agent or background-shell record. Protocol identifiers are kept
/// distinct even though the UI also exposes a compact preferred `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackgroundTask {
    pub(crate) id: String,
    pub(crate) kind: AgentWorkKind,
    pub(crate) task_id: Option<String>,
    pub(crate) child_session_id: Option<String>,
    pub(crate) agent_id: Option<String>,
    pub(crate) tool_call_id: Option<String>,
    pub(crate) tool: String,
    pub(crate) title: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) status: String,
    pub(crate) pid: Option<u64>,
    pub(crate) command: Option<String>,
    pub(crate) cancellable: bool,
    pub(crate) revision: Option<u64>,
}

impl BackgroundTask {
    fn from_snapshot(snapshot: AgentSnapshot) -> Option<Self> {
        let kind = if snapshot.kind == "background" {
            AgentWorkKind::Background
        } else {
            AgentWorkKind::Subagent
        };
        let id = preferred_id(kind, &snapshot)?;
        let title = snapshot.title.clone();
        Some(Self {
            id,
            kind,
            task_id: snapshot.task_id,
            child_session_id: snapshot.child_session_id,
            agent_id: snapshot.agent_id,
            tool_call_id: snapshot.tool_call_id,
            tool: title.clone().unwrap_or_else(|| match kind {
                AgentWorkKind::Subagent => "Subagent".to_string(),
                AgentWorkKind::Background => "Bash".to_string(),
            }),
            title,
            summary: snapshot.summary,
            status: snapshot.status.unwrap_or_else(|| "unknown".to_string()),
            pid: snapshot.pid,
            command: snapshot.command,
            cancellable: snapshot.cancellable.unwrap_or(false),
            revision: snapshot.revision,
        })
    }

    fn shares_identity(&self, other: &Self) -> bool {
        self.kind == other.kind
            && [
                (&self.task_id, &other.task_id),
                (&self.child_session_id, &other.child_session_id),
                (&self.agent_id, &other.agent_id),
                (&self.tool_call_id, &other.tool_call_id),
            ]
            .iter()
            .any(|(left, right)| left.is_some() && left == right)
    }

    fn merge(&mut self, incoming: Self) {
        merge_option(&mut self.task_id, incoming.task_id);
        merge_option(&mut self.child_session_id, incoming.child_session_id);
        merge_option(&mut self.agent_id, incoming.agent_id);
        merge_option(&mut self.tool_call_id, incoming.tool_call_id);
        merge_option(&mut self.title, incoming.title);
        merge_option(&mut self.summary, incoming.summary);
        merge_option(&mut self.command, incoming.command);
        if incoming.pid.is_some() {
            self.pid = incoming.pid;
        }
        self.cancellable = incoming.cancellable;

        let old_terminal = terminal_status(&self.status);
        let new_terminal = terminal_status(&incoming.status);
        let stale_revision = matches!(
            (self.revision, incoming.revision),
            (Some(old), Some(new)) if new < old
        );
        let preserves_terminal = !old_terminal || new_terminal;
        let revision_allows = !stale_revision || new_terminal && !old_terminal;
        if preserves_terminal && revision_allows {
            self.status = incoming.status;
            if incoming.tool != "Subagent" && incoming.tool != "Bash" {
                self.tool = incoming.tool;
            }
            self.revision = match (self.revision, incoming.revision) {
                (Some(old), Some(new)) => Some(old.max(new)),
                (old, new) => new.or(old),
            };
        }
        self.id = match self.kind {
            AgentWorkKind::Subagent => self
                .child_session_id
                .as_ref()
                .or(self.agent_id.as_ref())
                .or(self.task_id.as_ref())
                .or(self.tool_call_id.as_ref()),
            AgentWorkKind::Background => self
                .task_id
                .as_ref()
                .or(self.tool_call_id.as_ref())
                .or(self.agent_id.as_ref()),
        }
        .cloned()
        .unwrap_or_else(|| self.id.clone());
    }
}

fn merge_option<T>(target: &mut Option<T>, incoming: Option<T>) {
    if incoming.is_some() {
        *target = incoming;
    }
}

fn preferred_id(kind: AgentWorkKind, snapshot: &AgentSnapshot) -> Option<String> {
    match kind {
        AgentWorkKind::Subagent => snapshot
            .child_session_id
            .as_ref()
            .or(snapshot.agent_id.as_ref())
            .or(snapshot.task_id.as_ref())
            .or(snapshot.tool_call_id.as_ref()),
        AgentWorkKind::Background => snapshot
            .task_id
            .as_ref()
            .or(snapshot.tool_call_id.as_ref())
            .or(snapshot.agent_id.as_ref()),
    }
    .cloned()
}

fn terminal_status(status: &str) -> bool {
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

#[derive(Debug, Default)]
pub(crate) struct AgentInspectorState {
    tasks: Vec<BackgroundTask>,
    selected: Option<usize>,
}

impl AgentInspectorState {
    pub(crate) fn tasks(&self) -> &[BackgroundTask] {
        &self.tasks
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn is_open(&self) -> bool {
        self.selected.is_some()
    }

    pub(crate) fn open(&mut self) -> bool {
        if self.tasks.is_empty() {
            return false;
        }
        self.selected = Some(0);
        true
    }

    pub(crate) fn close(&mut self) {
        self.selected = None;
    }

    pub(crate) fn reset(&mut self) {
        self.tasks.clear();
        self.selected = None;
    }

    pub(crate) fn select_previous(&mut self) {
        if let Some(index) = &mut self.selected {
            *index = index.saturating_sub(1);
        }
    }

    pub(crate) fn select_next(&mut self) {
        if let Some(index) = &mut self.selected {
            *index = (*index + 1).min(self.tasks.len().saturating_sub(1));
        }
    }

    pub(crate) fn select_first(&mut self) {
        if self.selected.is_some() {
            self.selected = Some(0);
        }
    }

    pub(crate) fn select_last(&mut self) {
        if self.selected.is_some() {
            self.selected = Some(self.tasks.len().saturating_sub(1));
        }
    }

    pub(crate) fn merge_snapshots(&mut self, snapshots: Vec<AgentSnapshot>) {
        for snapshot in snapshots {
            if let Some(task) = BackgroundTask::from_snapshot(snapshot) {
                self.merge_task(task);
            }
        }
    }

    fn merge_task(&mut self, incoming: BackgroundTask) {
        if let Some(existing) = self
            .tasks
            .iter_mut()
            .find(|existing| existing.shares_identity(&incoming))
        {
            existing.merge(incoming);
        } else {
            self.tasks.insert(0, incoming);
        }
        if let Some(index) = &mut self.selected {
            *index = (*index).min(self.tasks.len().saturating_sub(1));
        }
    }

    /// Merge lifecycle events from both background Bash and Subagent domains.
    pub(crate) fn ingest(&mut self, event: &AppServerEvent) -> bool {
        let (kind, fallback_status) = match event.kind.as_str() {
            "background_task_started" => ("background", "running"),
            "background_task_updated" => ("background", "updated"),
            "background_task_completed" => ("background", "completed"),
            "subagent_spawned" | "subagent_started" => ("subagent", "running"),
            "subagent_message" | "subagent_updated" => ("subagent", "running"),
            "subagent_stopped" | "subagent_completed" => ("subagent", "completed"),
            _ => return false,
        };
        self.merge_snapshots(vec![AgentSnapshot {
            kind: kind.to_string(),
            task_id: event.task_id.clone(),
            child_session_id: event.child_session_id.clone(),
            agent_id: event.agent_id.clone(),
            tool_call_id: event.tool_call_id.clone(),
            title: event.title.clone().or_else(|| event.tool_name.clone()),
            summary: event.summary.clone(),
            status: Some(
                event
                    .status
                    .clone()
                    .unwrap_or_else(|| fallback_status.to_string()),
            ),
            command: event.command.clone(),
            pid: event.pid,
            cancellable: event.cancellable,
            revision: event.revision,
        }]);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_event_is_not_resurrected_by_late_running_update() {
        let mut state = AgentInspectorState::default();
        for (kind, status, revision) in [
            ("background_task_started", "running", 4),
            ("background_task_completed", "completed", 6),
            ("background_task_updated", "running", 5),
        ] {
            state.ingest(&AppServerEvent {
                kind: kind.to_string(),
                task_id: Some("bg-1".to_string()),
                status: Some(status.to_string()),
                revision: Some(revision),
                ..Default::default()
            });
        }
        assert_eq!(state.tasks[0].status, "completed");
        assert_eq!(state.tasks[0].revision, Some(6));
    }

    #[test]
    fn subagent_and_bash_ids_are_separate_domains() {
        let mut state = AgentInspectorState::default();
        state.merge_snapshots(vec![
            AgentSnapshot {
                kind: "subagent".to_string(),
                task_id: Some("shared".to_string()),
                child_session_id: Some("child-1".to_string()),
                status: Some("running".to_string()),
                ..Default::default()
            },
            AgentSnapshot {
                kind: "background".to_string(),
                task_id: Some("shared".to_string()),
                status: Some("running".to_string()),
                ..Default::default()
            },
        ]);
        assert_eq!(state.tasks.len(), 2);
        assert_ne!(state.tasks[0].kind, state.tasks[1].kind);
    }

    #[test]
    fn selection_stays_inside_the_task_list() {
        let mut state = AgentInspectorState::default();
        assert!(!state.open());
        state.ingest(&AppServerEvent {
            kind: "background_task_started".to_string(),
            task_id: Some("bg-1".to_string()),
            ..Default::default()
        });
        assert!(state.open());
        state.select_next();
        assert_eq!(state.selected(), Some(0));
        state.close();
        assert!(!state.is_open());
    }
}
