use zcode_tui::AppServerEvent;

/// A background task observed through the kernel's lifecycle-only events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackgroundTask {
    pub(crate) id: String,
    pub(crate) tool: String,
    pub(crate) status: String,
    pub(crate) pid: Option<u64>,
    pub(crate) command: Option<String>,
}

/// State owned by the read-only `/agents` inspector.
///
/// The kernel currently exposes lifecycle events rather than task transcripts
/// or controls, so this model deliberately keeps observation separate from UI
/// rendering and input routing.
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

    /// Merge the latest lifecycle event into the inspector cache.
    /// Returns whether the event belonged to the background-task domain.
    pub(crate) fn ingest(&mut self, event: &AppServerEvent) -> bool {
        if !matches!(
            event.kind.as_str(),
            "background_task_started" | "background_task_updated" | "background_task_completed"
        ) {
            return false;
        }
        let Some(id) = event.task_id.as_ref().or(event.tool_call_id.as_ref()) else {
            return true;
        };
        let fallback_status = match event.kind.as_str() {
            "background_task_started" => "running",
            "background_task_completed" => "completed",
            _ => "updated",
        };
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == *id) {
            task.status = event
                .status
                .clone()
                .unwrap_or_else(|| fallback_status.to_string());
            if let Some(tool) = &event.tool_name {
                task.tool.clone_from(tool);
            }
            if event.pid.is_some() {
                task.pid = event.pid;
            }
            if event.command.is_some() {
                task.command.clone_from(&event.command);
            }
        } else {
            self.tasks.insert(
                0,
                BackgroundTask {
                    id: id.clone(),
                    tool: event
                        .tool_name
                        .clone()
                        .unwrap_or_else(|| "task".to_string()),
                    status: event
                        .status
                        .clone()
                        .unwrap_or_else(|| fallback_status.to_string()),
                    pid: event.pid,
                    command: event.command.clone(),
                },
            );
        }
        if let Some(index) = &mut self.selected {
            *index = (*index).min(self.tasks.len().saturating_sub(1));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_events_merge_without_losing_known_details() {
        let mut state = AgentInspectorState::default();
        assert!(state.ingest(&AppServerEvent {
            kind: "background_task_started".to_string(),
            task_id: Some("bg-1".to_string()),
            tool_name: Some("Bash".to_string()),
            command: Some("sleep 12".to_string()),
            pid: Some(4242),
            ..Default::default()
        }));
        assert!(state.ingest(&AppServerEvent {
            kind: "background_task_completed".to_string(),
            task_id: Some("bg-1".to_string()),
            status: Some("completed".to_string()),
            ..Default::default()
        }));

        assert_eq!(state.tasks.len(), 1);
        assert_eq!(state.tasks[0].status, "completed");
        assert_eq!(state.tasks[0].pid, Some(4242));
        assert_eq!(state.tasks[0].command.as_deref(), Some("sleep 12"));
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
