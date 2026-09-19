//! Browser Use preflight and diagnostics; never launches or installs a browser.

use std::path::{Path, PathBuf};

use crate::{DbBaseline, LiveToolChip, ToolChipStatus};
use anyhow::{bail, Result};

fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Match the official CLI's explicit-path contract, without running the file.
pub fn validate_browser_executable(value: &str) -> Result<()> {
    let path = Path::new(value);
    if !path.is_absolute() {
        bail!("Browser Use: --browser-executable requires an absolute Chrome/Chromium path");
    }
    if !executable_file(path) {
        bail!("Browser Use: executable missing or not executable: {value}; install Chrome/Chromium or correct --browser-executable");
    }
    Ok(())
}

fn browser_candidates() -> Vec<PathBuf> {
    match std::env::consts::OS {
        "macos" => [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect(),
        "windows" => ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(std::env::var_os)
            .flat_map(|root| {
                [
                    "Google/Chrome/Application/chrome.exe",
                    "Chromium/Application/chrome.exe",
                    "Microsoft/Edge/Application/msedge.exe",
                ]
                .map(|suffix| PathBuf::from(&root).join(suffix))
            })
            .collect(),
        _ => [
            "/usr/bin/google-chrome-stable",
            "/usr/bin/google-chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect(),
    }
}

pub fn browser_preflight(explicit: Option<&str>) -> Result<String> {
    if let Some(path) = explicit {
        validate_browser_executable(path)?;
        return Ok(format!(
            "Browser Use: executable found: {path} (launch not yet verified)"
        ));
    }
    // Informational only: the kernel may prefer its own managed Chromium.
    Ok(match browser_candidates().into_iter().find(|path| executable_file(path)) {
        Some(path) => format!("Browser Use: system candidate {} found; the official kernel selects and launches the browser", path.display()),
        None => "Browser Use: no system Chrome/Chromium found; the kernel may use its managed browser. If unavailable, install Chrome/Chromium and set --browser-executable <absolute-path>".into(),
    })
}

/// Recognize only known runtime failures, never infer browser success from stdout.
pub fn browser_failure_hint(message: &str) -> Option<&'static str> {
    if message.contains("failed to load the pinned Playwright runtime")
        || message.contains("bundled runtime repair failed")
    {
        Some("Browser Use: official Playwright runtime unavailable. Reinstall the managed zcode wrapper with install.sh; it resolves the matching runtime from app.asar without changing the official package.")
    } else if message.contains("No permission client configured") {
        Some("Browser Use: this CLI route cannot ask for tool approval. Use the official desktop client for interactive approval; permissions will not be relaxed automatically.")
    } else if message.contains("Unknown option '--allowed-tools'") {
        Some("Browser Use: this official kernel rejects --allowed-tools despite advertising it. The allowlist will not be silently discarded; use the official desktop client for permission-controlled browser tasks.")
    } else if message.contains("No installed Chrome or Chromium executable")
        || message.contains("Browser executable is missing or not executable")
    {
        Some("Browser Use: Chrome/Chromium is unavailable. Install it or set --browser-executable to an existing absolute executable path.")
    } else if message.contains("Managed headless Chromium is unavailable: launch failed") {
        Some("Browser Use: Chromium launch failed. Check browser/OS runtime dependencies and sandbox support; no sandbox flags are changed automatically.")
    } else if message.contains("Browser plugin root is unavailable") {
        Some("Browser Use: official browser plugin unavailable. Check the browser-use plugin in the matching official ZCode package.")
    } else if message.contains("ZCode Built-in missing") {
        Some("Browser Use: the official kernel could not register its built-in node_repl/browser service. Re-run install.sh to refresh the wrapper and plugin cache; if it persists on ZCode 3.14.x, use the official Desktop client or downgrade to a verified kernel.")
    } else if message.contains("MCP tool returned an error:") {
        Some("Browser Use: the official browser tool reported an error. Check the tool result and browser-use skill; a successful CLI exit does not prove browser success.")
    } else {
        None
    }
}

/// Read only tool results belonging to this turn, not prompts or tool input code.
pub fn browser_failure_from_db(
    conn: &rusqlite::Connection,
    session: &str,
    baseline: DbBaseline,
) -> Option<&'static str> {
    let mut stmt = conn
        .prepare(
            "SELECT json_extract(data, '$.state.error'), json_extract(data, '$.state.output') \
         FROM part WHERE session_id = ?1 AND rowid > ?2 \
         AND json_extract(data, '$.type') = 'tool' \
         AND json_extract(data, '$.tool') IN ('mcp__node_repl__js', 'js') \
         ORDER BY rowid DESC LIMIT 64",
        )
        .ok()?;
    let rows = stmt
        .query_map(rusqlite::params![session, baseline.part_rowid], |row| {
            Ok((
                row.get::<_, Option<String>>(0).unwrap_or_default(),
                row.get::<_, Option<String>>(1).unwrap_or_default(),
            ))
        })
        .ok()?;
    for (error, output) in rows.flatten() {
        if let Some(hint) = error.as_deref().and_then(browser_failure_hint) {
            return Some(hint);
        }
        if let Some(message) = output {
            // A successful DOM snapshot may quote error messages as page text.
            // Only the official failure envelope/prefix is diagnostic evidence.
            if !message.starts_with("MCP tool returned an error:")
                && !message.starts_with("No permission client configured")
            {
                continue;
            }
            if let Some(hint) = browser_failure_hint(&message) {
                return Some(hint);
            }
        }
    }
    None
}

pub fn browser_progress(chips: &[LiveToolChip], elapsed: f32, cancelling: bool) -> String {
    let phase = if cancelling {
        "cancelling"
    } else if chips
        .iter()
        .any(|chip| chip.status == ToolChipStatus::Running)
    {
        "tool running"
    } else if chips
        .iter()
        .any(|chip| chip.status == ToolChipStatus::Failed)
    {
        "tool error reported"
    } else if chips.is_empty() {
        "waiting for model/tools"
    } else {
        "waiting for final response"
    };
    format!("Browser Use · {phase} · {elapsed:.1}s · Esc cancel")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_browser_must_be_absolute_existing_and_executable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(validate_browser_executable("chrome").is_err());
        assert!(validate_browser_executable(dir.path().to_str().unwrap()).is_err());
        let file = dir.path().join("chrome with spaces");
        assert!(validate_browser_executable(file.to_str().unwrap()).is_err());
        std::fs::write(&file, "not executed by preflight").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(validate_browser_executable(file.to_str().unwrap()).is_err());
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(browser_preflight(Some(file.to_str().unwrap()))
            .unwrap()
            .contains("launch not yet verified"));
    }

    #[test]
    fn diagnostics_distinguish_runtime_browser_and_permissions() {
        for (error, hint) in [
            ("Managed headless Chromium is unavailable: failed to load the pinned Playwright runtime.", "app.asar"),
            ("No permission client configured for mcp__node_repl__js", "will not be relaxed"),
            ("No installed Chrome or Chromium executable was found.", "absolute"),
            ("Managed headless Chromium is unavailable: launch failed.", "sandbox"),
            ("Browser plugin root is unavailable in the node_repl host", "plugin"),
            ("ZCode Built-in missing", "built-in node_repl/browser service"),
        ] {
            assert!(browser_failure_hint(error).unwrap().contains(hint));
        }
        assert!(browser_failure_hint("navigation completed").is_none());
        assert!(browser_failure_hint("401 unauthorized").is_none());
    }

    #[test]
    fn progress_uses_observations_not_fabricated_browser_success() {
        assert!(browser_progress(&[], 1.5, false).contains("waiting for model/tools"));
        let mut chip = LiveToolChip {
            tool: "mcp__node_repl__js".into(),
            status: ToolChipStatus::Running,
            duration_ms: None,
        };
        assert!(browser_progress(&[chip.clone()], 2.0, false).contains("tool running"));
        chip.status = ToolChipStatus::Completed;
        assert!(
            browser_progress(&[chip.clone()], 3.0, false).contains("waiting for final response")
        );
        chip.status = ToolChipStatus::Failed;
        assert!(browser_progress(&[chip.clone()], 3.0, false).contains("tool error"));
        assert!(browser_progress(&[chip], 3.0, true).contains("cancelling"));
    }

    #[test]
    fn db_diagnostics_ignore_other_sessions_old_rows_and_tool_input() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE part (session_id TEXT, data TEXT)")
            .unwrap();
        let error = "No permission client configured for mcp__node_repl__js";
        let insert = |session: &str, input: &str, output: &str| {
            let data = serde_json::json!({"type":"tool","tool":"mcp__node_repl__js","state":{"status":"completed","input":{"code":input},"output":output}});
            conn.execute(
                "INSERT INTO part VALUES (?1, ?2)",
                rusqlite::params![session, data.to_string()],
            )
            .unwrap();
        };
        insert("parent", "", error);
        let baseline = DbBaseline {
            part_rowid: 1,
            tool_rowid: 0,
        };
        insert("other", "", error);
        insert("parent", error, "success");
        insert("parent", "", &format!("- heading: {error}"));
        assert!(browser_failure_from_db(&conn, "parent", baseline).is_none());
        // Official MCP failures can have state.status=completed and exit code 0.
        insert("parent", "", error);
        assert!(browser_failure_from_db(&conn, "parent", baseline)
            .unwrap()
            .contains("approval"));
    }
}
