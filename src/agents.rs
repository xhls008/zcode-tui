use std::collections::HashSet;

use zcode_tui::{AgentSnapshot, AppServerEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentWorkKind {
    Subagent,
    Background,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum InspectorTab {
    #[default]
    Agents,
    Background,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum InspectorView {
    #[default]
    List,
    Detail,
}

const PARENT_KEY: &str = "parent";
const OUTPUT_TAIL_CHARS: usize = 16_000;

fn bounded_tail(value: &str) -> String {
    let count = value.chars().count();
    value
        .chars()
        .skip(count.saturating_sub(OUTPUT_TAIL_CHARS))
        .collect()
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
    pub(crate) output_tail: Option<String>,
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
            output_tail: snapshot.output_tail,
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
        if let Some(output) = incoming.output_tail {
            self.output_tail = Some(bounded_tail(&output));
        }
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

    fn append_output(&mut self, delta: &str) {
        if delta.is_empty()
            || self
                .output_tail
                .as_deref()
                .is_some_and(|output| output.ends_with(delta))
        {
            return;
        }
        let mut output = self.output_tail.take().unwrap_or_default();
        output.push_str(delta);
        self.output_tail = Some(bounded_tail(&output));
    }

    fn inspector_key(&self) -> String {
        let prefix = match self.kind {
            AgentWorkKind::Subagent => "agent",
            AgentWorkKind::Background => "background",
        };
        format!("{prefix}:{}", self.id)
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
    open: bool,
    tab: InspectorTab,
    view: InspectorView,
    selected_key: Option<String>,
    detail_scroll: u16,
    cancel_in_flight: HashSet<String>,
    refresh_in_flight: bool,
}

impl AgentInspectorState {
    pub(crate) fn tasks(&self) -> &[BackgroundTask] {
        &self.tasks
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        if !self.open {
            return None;
        }
        let selected = self.selected_key.as_deref()?;
        self.visible_keys().iter().position(|key| key == selected)
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn is_refreshing(&self) -> bool {
        self.refresh_in_flight
    }

    pub(crate) fn begin_refresh(&mut self) -> bool {
        if self.refresh_in_flight {
            return false;
        }
        self.refresh_in_flight = true;
        true
    }

    pub(crate) fn finish_refresh(&mut self) {
        self.refresh_in_flight = false;
    }

    pub(crate) fn open(&mut self) -> bool {
        self.open = true;
        self.tab = InspectorTab::Agents;
        self.view = InspectorView::List;
        self.selected_key = Some(PARENT_KEY.to_string());
        self.detail_scroll = 0;
        true
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.selected_key = None;
        self.detail_scroll = 0;
    }

    pub(crate) fn reset(&mut self) {
        self.tasks.clear();
        self.cancel_in_flight.clear();
        self.refresh_in_flight = false;
        self.close();
    }

    pub(crate) fn tab(&self) -> InspectorTab {
        self.tab
    }

    pub(crate) fn view(&self) -> InspectorView {
        self.view
    }

    pub(crate) fn detail_scroll(&self) -> u16 {
        self.detail_scroll
    }

    pub(crate) fn selected_is_parent(&self) -> bool {
        self.tab == InspectorTab::Agents && self.selected_key.as_deref() == Some(PARENT_KEY)
    }

    pub(crate) fn selected_task(&self) -> Option<&BackgroundTask> {
        let key = self.selected_key.as_deref()?;
        self.tasks.iter().find(|task| task.inspector_key() == key)
    }

    pub(crate) fn visible_tasks(&self) -> Vec<&BackgroundTask> {
        let kind = match self.tab {
            InspectorTab::Agents => AgentWorkKind::Subagent,
            InspectorTab::Background => AgentWorkKind::Background,
        };
        self.tasks.iter().filter(|task| task.kind == kind).collect()
    }

    pub(crate) fn linked_background<'a>(
        &'a self,
        agent: &'a BackgroundTask,
    ) -> Vec<&'a BackgroundTask> {
        let Some(agent_id) = agent.agent_id.as_ref() else {
            return Vec::new();
        };
        self.tasks
            .iter()
            .filter(|task| {
                task.kind == AgentWorkKind::Background && task.agent_id.as_ref() == Some(agent_id)
            })
            .collect()
    }

    pub(crate) fn cancel_pending(&self, task_id: &str) -> bool {
        self.cancel_in_flight.contains(task_id)
    }

    pub(crate) fn selected_cancel_eligible(&self) -> bool {
        self.selected_task().is_some_and(|task| {
            task.kind == AgentWorkKind::Background
                && task.cancellable
                && task.task_id.is_some()
                && !terminal_status(&task.status)
                && !self
                    .cancel_in_flight
                    .contains(task.task_id.as_deref().unwrap_or_default())
        })
    }

    pub(crate) fn begin_cancel_selected(&mut self) -> Option<String> {
        if !self.selected_cancel_eligible() {
            return None;
        }
        let task_id = self.selected_task()?.task_id.clone()?;
        self.cancel_in_flight.insert(task_id.clone());
        Some(task_id)
    }

    pub(crate) fn finish_cancel(&mut self, task_id: &str) {
        self.cancel_in_flight.remove(task_id);
    }

    pub(crate) fn task_is_terminal(&self, task_id: &str) -> bool {
        self.tasks
            .iter()
            .any(|task| task.task_id.as_deref() == Some(task_id) && terminal_status(&task.status))
    }

    fn visible_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if self.tab == InspectorTab::Agents {
            keys.push(PARENT_KEY.to_string());
        }
        keys.extend(
            self.visible_tasks()
                .into_iter()
                .map(BackgroundTask::inspector_key),
        );
        keys
    }

    pub(crate) fn toggle_tab(&mut self) {
        self.tab = match self.tab {
            InspectorTab::Agents => InspectorTab::Background,
            InspectorTab::Background => InspectorTab::Agents,
        };
        self.view = InspectorView::List;
        self.detail_scroll = 0;
        self.selected_key = self.visible_keys().into_iter().next();
    }

    pub(crate) fn open_detail(&mut self) {
        if self.selected_key.is_some() {
            self.view = InspectorView::Detail;
            self.detail_scroll = 0;
        }
    }

    pub(crate) fn back_to_list(&mut self) -> bool {
        if self.view == InspectorView::Detail {
            self.view = InspectorView::List;
            self.detail_scroll = 0;
            true
        } else {
            false
        }
    }

    pub(crate) fn scroll_detail(&mut self, delta: i16) {
        self.detail_scroll = if delta < 0 {
            self.detail_scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.detail_scroll.saturating_add(delta as u16)
        };
    }

    pub(crate) fn select_previous(&mut self) {
        let keys = self.visible_keys();
        if let Some(index) = self.selected() {
            self.selected_key = keys.get(index.saturating_sub(1)).cloned();
        }
    }

    pub(crate) fn select_next(&mut self) {
        let keys = self.visible_keys();
        if let Some(index) = self.selected() {
            self.selected_key = keys
                .get((index + 1).min(keys.len().saturating_sub(1)))
                .cloned();
        }
    }

    pub(crate) fn select_first(&mut self) {
        if self.open {
            self.selected_key = self.visible_keys().into_iter().next();
        }
    }

    pub(crate) fn select_last(&mut self) {
        if self.open {
            self.selected_key = self.visible_keys().into_iter().last();
        }
    }

    pub(crate) fn merge_snapshots(&mut self, snapshots: Vec<AgentSnapshot>) {
        for snapshot in snapshots {
            if let Some(task) = BackgroundTask::from_snapshot(snapshot) {
                self.merge_task(task);
            }
        }
    }

    fn merge_task(&mut self, incoming: BackgroundTask) -> String {
        let mut settled_task_id = None;
        let key;
        if let Some(existing) = self
            .tasks
            .iter_mut()
            .find(|existing| existing.shares_identity(&incoming))
        {
            let old_key = existing.inspector_key();
            existing.merge(incoming);
            if self.selected_key.as_deref() == Some(old_key.as_str()) {
                self.selected_key = Some(existing.inspector_key());
            }
            if terminal_status(&existing.status) {
                settled_task_id.clone_from(&existing.task_id);
            }
            key = existing.inspector_key();
        } else {
            if terminal_status(&incoming.status) {
                settled_task_id.clone_from(&incoming.task_id);
            }
            key = incoming.inspector_key();
            self.tasks.insert(0, incoming);
        }
        if let Some(task_id) = settled_task_id {
            self.cancel_in_flight.remove(&task_id);
        }
        if self.open && self.selected().is_none() {
            self.selected_key = self.visible_keys().into_iter().next();
        }
        key
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
        let progress = matches!(event.kind.as_str(), "subagent_message" | "subagent_updated");
        let output_delta = progress
            .then(|| event.output.as_deref().or(event.summary.as_deref()))
            .flatten()
            .map(str::to_string);
        let Some(task) = BackgroundTask::from_snapshot(AgentSnapshot {
            kind: kind.to_string(),
            task_id: event.task_id.clone(),
            child_session_id: event.child_session_id.clone(),
            agent_id: event.agent_id.clone(),
            tool_call_id: event.tool_call_id.clone(),
            title: event.title.clone().or_else(|| event.tool_name.clone()),
            summary: (!progress).then(|| event.summary.clone()).flatten(),
            status: Some(
                event
                    .status
                    .clone()
                    .unwrap_or_else(|| fallback_status.to_string()),
            ),
            command: event.command.clone(),
            output_tail: (!progress).then(|| event.output.clone()).flatten(),
            pid: event.pid,
            cancellable: event.cancellable,
            revision: event.revision,
        }) else {
            return false;
        };
        let key = self.merge_task(task);
        if let Some(delta) = output_delta {
            if let Some(task) = self
                .tasks
                .iter_mut()
                .find(|task| task.inspector_key() == key)
            {
                task.append_output(&delta);
            }
        }
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
        assert!(state.open());
        assert!(state.selected_is_parent());
        state.toggle_tab();
        assert_eq!(state.tab(), InspectorTab::Background);
        assert_eq!(state.selected(), None);
        state.ingest(&AppServerEvent {
            kind: "background_task_started".to_string(),
            task_id: Some("bg-1".to_string()),
            ..Default::default()
        });
        assert_eq!(state.selected(), Some(0));
        state.select_next();
        assert_eq!(state.selected(), Some(0));
        state.close();
        assert!(!state.is_open());
    }

    #[test]
    fn live_insert_preserves_selected_record_and_detail_scroll() {
        let mut state = AgentInspectorState::default();
        state.merge_snapshots(vec![AgentSnapshot {
            kind: "subagent".to_string(),
            child_session_id: Some("child-1".to_string()),
            status: Some("running".to_string()),
            ..Default::default()
        }]);
        state.open();
        state.select_next();
        state.open_detail();
        state.scroll_detail(7);
        state.merge_snapshots(vec![AgentSnapshot {
            kind: "subagent".to_string(),
            child_session_id: Some("child-2".to_string()),
            status: Some("running".to_string()),
            ..Default::default()
        }]);
        assert_eq!(
            state.selected_task().map(|task| task.id.as_str()),
            Some("child-1")
        );
        assert_eq!(state.view(), InspectorView::Detail);
        assert_eq!(state.detail_scroll(), 7);
    }

    #[test]
    fn cancel_uses_only_eligible_task_id_and_completion_wins_race() {
        let mut state = AgentInspectorState::default();
        state.merge_snapshots(vec![AgentSnapshot {
            kind: "background".to_string(),
            task_id: Some("task-exact".to_string()),
            child_session_id: Some("child-wrong".to_string()),
            agent_id: Some("agent-wrong".to_string()),
            tool_call_id: Some("call-wrong".to_string()),
            status: Some("running".to_string()),
            cancellable: Some(true),
            ..Default::default()
        }]);
        state.open();
        state.toggle_tab();
        assert!(state.selected_cancel_eligible());
        assert_eq!(state.begin_cancel_selected().as_deref(), Some("task-exact"));
        assert_eq!(state.begin_cancel_selected(), None);
        assert_eq!(state.selected_task().unwrap().status, "running");

        state.ingest(&AppServerEvent {
            kind: "background_task_completed".to_string(),
            task_id: Some("task-exact".to_string()),
            status: Some("completed".to_string()),
            ..Default::default()
        });
        assert!(!state.cancel_pending("task-exact"));
        assert!(state.task_is_terminal("task-exact"));
        assert!(!state.selected_cancel_eligible());
    }

    #[test]
    fn subagent_progress_appends_without_replacing_final_summary() {
        let mut state = AgentInspectorState::default();
        state.merge_snapshots(vec![AgentSnapshot {
            kind: "subagent".to_string(),
            child_session_id: Some("child-live".to_string()),
            agent_id: Some("agent-live".to_string()),
            summary: Some("final result remains authoritative".to_string()),
            status: Some("success".to_string()),
            ..Default::default()
        }]);

        state.ingest(&AppServerEvent {
            kind: "subagent_message".to_string(),
            child_session_id: Some("child-live".to_string()),
            agent_id: Some("agent-live".to_string()),
            output: Some("first update".to_string()),
            ..Default::default()
        });
        state.ingest(&AppServerEvent {
            kind: "subagent_message".to_string(),
            child_session_id: Some("child-live".to_string()),
            agent_id: Some("agent-live".to_string()),
            output: Some(" + second update".to_string()),
            ..Default::default()
        });

        let task = &state.tasks[0];
        assert_eq!(
            task.summary.as_deref(),
            Some("final result remains authoritative")
        );
        assert_eq!(
            task.output_tail.as_deref(),
            Some("first update + second update")
        );
        assert_eq!(task.status, "success");
    }

    #[test]
    fn refresh_state_is_single_flight_and_resettable() {
        let mut state = AgentInspectorState::default();
        assert!(state.begin_refresh());
        assert!(state.is_refreshing());
        assert!(!state.begin_refresh());
        state.finish_refresh();
        assert!(!state.is_refreshing());
        assert!(state.begin_refresh());
        state.reset();
        assert!(!state.is_refreshing());
    }
}
