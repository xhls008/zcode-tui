use std::fs;
use std::path::PathBuf;

use zcode_tui::{
    build_prompt_command, build_prompt_command_with_attachments, classify_input,
    command_palette_rows, detect_auth_status_with, extract_file_mentions, file_suggestions,
    handle_local_command, leader_action_for_key, load_mcp_config, login_command, logout_command,
    mask_secret, parse_cli_args, save_mcp_config, slash_suggestions, strip_ansi,
    user_mcp_config_path_from, AppConfig, AuthStatus, InputAction, LeaderAction, McpServer,
};

#[test]
fn parse_args_ignores_tui_and_preserves_session_options() {
    let config = parse_cli_args([
        "tui",
        "--cwd",
        "/tmp/project",
        "--mode",
        "plan",
        "--resume",
        "sess_123",
        "--no-color",
    ])
    .unwrap();

    assert_eq!(config.cwd.as_deref(), Some("/tmp/project"));
    assert_eq!(config.mode.as_deref(), Some("plan"));
    assert_eq!(config.resume.as_deref(), Some("sess_123"));
    assert!(!config.continue_session);
    assert!(config.no_color);
}

#[test]
fn build_prompt_command_uses_headless_zcode_cli_options() {
    let config = parse_cli_args([
        "--cwd",
        "/tmp/project",
        "--mode",
        "edit",
        "--continue",
        "--attach",
        "/tmp/a.txt",
        "--attach=/tmp/b.txt",
    ])
    .unwrap();

    let command = build_prompt_command("/usr/local/bin/zcode", &config, "explain this");

    assert_eq!(
        command,
        vec![
            "/usr/local/bin/zcode",
            "--cwd",
            "/tmp/project",
            "--mode",
            "edit",
            "--continue",
            "--attach",
            "/tmp/a.txt",
            "--attach",
            "/tmp/b.txt",
            "--prompt",
            "explain this",
        ]
    );
}

#[test]
fn build_prompt_command_appends_mention_attachments() {
    let config = AppConfig::default();
    let command = build_prompt_command_with_attachments(
        "zcode",
        &config,
        "review @src/lib.rs",
        &["src/lib.rs".to_string()],
    );

    assert_eq!(
        command,
        vec![
            "zcode",
            "--attach",
            "src/lib.rs",
            "--prompt",
            "review @src/lib.rs",
        ]
    );
}

#[test]
fn extract_file_mentions_keeps_only_existing_files() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/lib.rs"), "pub fn x() {}\n").unwrap();

    let mentions = extract_file_mentions(
        "compare @src/lib.rs, with @missing.rs and @src/lib.rs again",
        temp.path(),
    );

    assert_eq!(mentions, vec!["src/lib.rs".to_string()]);
}

#[test]
fn extract_file_mentions_rejects_paths_outside_cwd() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(temp.path().join("secret.txt"), "outside\n").unwrap();
    fs::write(project.join("inside.txt"), "inside\n").unwrap();

    // Escapes via ../, absolute paths, and directories are all rejected.
    let mentions = extract_file_mentions("@../secret.txt @/etc/passwd @inside.txt @.", &project);
    assert_eq!(mentions, vec!["inside.txt".to_string()]);

    // A symlink inside cwd pointing outside is rejected after resolution.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path().join("secret.txt"), project.join("leak.txt"))
            .unwrap();
        assert!(extract_file_mentions("@leak.txt", &project).is_empty());
    }
}

#[test]
fn target_option_becomes_initial_goal_prompt() {
    let config = parse_cli_args(["--target", "audit filesystem", "--target-replace"]).unwrap();

    assert_eq!(
        config.initial_prompts,
        vec!["/goal replace audit filesystem".to_string()]
    );
}

#[test]
fn classify_goal_and_skill_commands() {
    assert_eq!(
        classify_input("/goal audit firmware").unwrap(),
        InputAction::Prompt("/goal audit firmware".to_string())
    );
    assert_eq!(
        classify_input("/skill pdf make a report").unwrap(),
        InputAction::Prompt("/skill pdf make a report".to_string())
    );
    assert_eq!(
        classify_input("/skills list").unwrap(),
        InputAction::Local(vec!["skills".into(), "list".into()])
    );
}

#[test]
fn classify_auth_commands_as_local() {
    assert_eq!(
        classify_input("/login").unwrap(),
        InputAction::Local(vec!["login".into()])
    );
    assert_eq!(
        classify_input("/logout").unwrap(),
        InputAction::Local(vec!["logout".into()])
    );
    assert_eq!(
        classify_input("/auth").unwrap(),
        InputAction::Local(vec!["auth".into()])
    );
    assert_eq!(
        classify_input("/status").unwrap(),
        InputAction::Local(vec!["status".into()])
    );
}

#[test]
fn classify_bang_commands_as_local_shell() {
    assert_eq!(
        classify_input("! cargo test").unwrap(),
        InputAction::Shell("cargo test".to_string())
    );
    assert_eq!(classify_input("!   ").unwrap(), InputAction::Empty);
}

#[test]
fn slash_suggestions_filter_by_prefix() {
    let suggestions = slash_suggestions("/mc", 5);

    assert_eq!(suggestions[0].command, "/mcp list");
    assert!(suggestions.iter().any(|item| item.command == "/mcp add"));
    assert!(suggestions
        .iter()
        .all(|item| item.command.starts_with("/mc")));
}

#[test]
fn slash_suggestions_fall_back_to_fuzzy_matches() {
    let suggestions = slash_suggestions("/mrm", 10);
    assert!(suggestions.iter().any(|item| item.command == "/mcp remove"));

    let suggestions = slash_suggestions("/lgn", 10);
    assert!(suggestions.iter().any(|item| item.command == "/login"));
}

#[test]
fn file_suggestions_match_and_skip_build_dirs() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::create_dir(temp.path().join("target")).unwrap();
    fs::write(temp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(temp.path().join("README.md"), "# hi\n").unwrap();
    fs::write(temp.path().join("target/skip.rs"), "").unwrap();

    let suggestions = file_suggestions(temp.path(), "main", 10);
    assert_eq!(suggestions, vec!["src/main.rs".to_string()]);

    let all = file_suggestions(temp.path(), "", 10);
    assert!(all.contains(&"README.md".to_string()));
    assert!(all.contains(&"src/".to_string()));
    assert!(!all.iter().any(|path| path.contains("target")));
}

#[test]
fn command_palette_exposes_common_commands() {
    let rows = command_palette_rows();

    assert!(rows.iter().any(|row| row.contains("/goal")));
    assert!(rows.iter().any(|row| row.contains("/skills list")));
    assert!(rows.iter().any(|row| row.contains("/login")));
    assert!(rows.iter().any(|row| row.contains("! <cmd>")));
}

#[test]
fn leader_keys_map_to_tui_actions() {
    assert_eq!(
        leader_action_for_key('p'),
        Some(LeaderAction::CommandPalette)
    );
    assert_eq!(leader_action_for_key('h'), Some(LeaderAction::Help));
    assert_eq!(leader_action_for_key('e'), Some(LeaderAction::Editor));
    assert_eq!(
        leader_action_for_key('x'),
        Some(LeaderAction::ClearConversation)
    );
    assert_eq!(leader_action_for_key('z'), None);
}

#[test]
fn classify_mcp_config_commands_as_local_and_status_as_prompt() {
    assert_eq!(
        classify_input("/mcp list").unwrap(),
        InputAction::Local(vec!["mcp".into(), "list".into()])
    );
    assert_eq!(
        classify_input("/mcp add fs npx -y @modelcontextprotocol/server-filesystem /tmp").unwrap(),
        InputAction::Local(vec![
            "mcp".into(),
            "add".into(),
            "fs".into(),
            "npx".into(),
            "-y".into(),
            "@modelcontextprotocol/server-filesystem".into(),
            "/tmp".into(),
        ])
    );
    assert_eq!(
        classify_input("/mcp get fs").unwrap(),
        InputAction::Local(vec!["mcp".into(), "get".into(), "fs".into()])
    );
    assert_eq!(
        classify_input("/mcp disable fs").unwrap(),
        InputAction::Local(vec!["mcp".into(), "disable".into(), "fs".into()])
    );
    assert_eq!(
        classify_input("/mcp status").unwrap(),
        InputAction::Prompt("/mcp status".to_string())
    );
}

#[test]
fn mcp_config_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join(".mcp.json");
    let mut config = load_mcp_config(&config_path).unwrap();

    config.servers.insert(
        "fs".to_string(),
        McpServer {
            command: "npx".to_string(),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
                temp.path().display().to_string(),
            ],
            env: [("NODE_ENV".to_string(), "test".to_string())].into(),
            ..Default::default()
        },
    );
    save_mcp_config(&config_path, &config).unwrap();

    let raw = fs::read_to_string(&config_path).unwrap();
    assert!(raw.contains("\"mcpServers\""));
    assert!(raw.contains("\"fs\""));
    assert!(raw.contains("@modelcontextprotocol/server-filesystem"));
    assert!(!raw.contains("\"disabled\""));

    let reloaded = load_mcp_config(&config_path).unwrap();
    assert_eq!(reloaded.servers["fs"].command, "npx");
    assert_eq!(reloaded.servers["fs"].transport_label(), "stdio");
    assert_eq!(
        reloaded.servers["fs"].args.last().unwrap(),
        &temp.path().display().to_string()
    );
}

fn project_config(path: &std::path::Path) -> AppConfig {
    AppConfig {
        cwd: Some(path.display().to_string()),
        ..Default::default()
    }
}

#[test]
fn mcp_add_remote_transport_and_disable_via_local_commands() {
    let temp = tempfile::tempdir().unwrap();
    let config = project_config(temp.path());

    let added = handle_local_command(
        &[
            "mcp".to_string(),
            "add".to_string(),
            "--transport".to_string(),
            "http".to_string(),
            "linear".to_string(),
            "https://mcp.linear.app/mcp".to_string(),
        ],
        &config,
        "zcode",
    )
    .unwrap();
    assert!(added.contains("linear"));
    assert!(added.contains("[project]"));

    let loaded = load_mcp_config(&temp.path().join(".mcp.json")).unwrap();
    assert_eq!(loaded.servers["linear"].transport.as_deref(), Some("http"));
    assert_eq!(
        loaded.servers["linear"].url.as_deref(),
        Some("https://mcp.linear.app/mcp")
    );
    assert!(loaded.servers["linear"].command.is_empty());

    let disabled = handle_local_command(
        &[
            "mcp".to_string(),
            "disable".to_string(),
            "linear".to_string(),
        ],
        &config,
        "zcode",
    )
    .unwrap();
    assert!(disabled.contains("Disabled"));
    let loaded = load_mcp_config(&temp.path().join(".mcp.json")).unwrap();
    assert!(loaded.servers["linear"].disabled);

    let listed =
        handle_local_command(&["mcp".to_string(), "list".to_string()], &config, "zcode").unwrap();
    assert!(listed.contains("[http] https://mcp.linear.app/mcp (disabled)"));
}

#[test]
fn mcp_add_preserves_server_flags_after_command() {
    let temp = tempfile::tempdir().unwrap();
    let config = project_config(temp.path());

    // Flags after the server command belong to the server, not the wrapper.
    handle_local_command(
        &[
            "mcp".to_string(),
            "add".to_string(),
            "fs".to_string(),
            "npx".to_string(),
            "--user".to_string(),
            "--transport".to_string(),
            "x".to_string(),
        ],
        &config,
        "zcode",
    )
    .unwrap();
    let loaded = load_mcp_config(&temp.path().join(".mcp.json")).unwrap();
    assert_eq!(loaded.servers["fs"].command, "npx");
    assert_eq!(
        loaded.servers["fs"].args,
        vec!["--user", "--transport", "x"]
    );

    // Everything after a literal -- is verbatim, even before 3 positionals.
    handle_local_command(
        &[
            "mcp".to_string(),
            "add".to_string(),
            "srv".to_string(),
            "--".to_string(),
            "runner".to_string(),
            "--scope".to_string(),
            "value".to_string(),
        ],
        &config,
        "zcode",
    )
    .unwrap();
    let loaded = load_mcp_config(&temp.path().join(".mcp.json")).unwrap();
    assert_eq!(loaded.servers["srv"].command, "runner");
    assert_eq!(loaded.servers["srv"].args, vec!["--scope", "value"]);
}

#[test]
fn streaming_job_delivers_all_output_with_eof_signals() {
    use std::time::Duration;
    use zcode_tui::{spawn_streaming_command, JobEvent};

    let job = spawn_streaming_command(&[
        "sh".to_string(),
        "-c".to_string(),
        "seq 1 500; echo tail-marker >&2".to_string(),
    ])
    .unwrap();
    assert_eq!(job.streams, 2);

    let mut lines = Vec::new();
    let mut eofs = 0;
    let mut finished = false;
    while let Ok(event) = job.receiver.recv_timeout(Duration::from_secs(10)) {
        match event {
            JobEvent::Line(line) => lines.push(line),
            JobEvent::Eof => eofs += 1,
            JobEvent::Finished { success, .. } => {
                assert!(success);
                finished = true;
            }
        }
    }

    assert!(finished);
    assert_eq!(eofs, job.streams);
    assert_eq!(lines.iter().filter(|l| l.as_str() == "500").count(), 1);
    assert!(lines.iter().any(|l| l == "tail-marker"));
    assert_eq!(lines.len(), 501);
}

#[test]
fn mcp_add_json_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let config = project_config(temp.path());

    handle_local_command(
        &[
            "mcp".to_string(),
            "add-json".to_string(),
            "docs".to_string(),
            r#"{"type":"sse","url":"https://example.com/sse","headers":{"X-Api-Key":"k"}}"#
                .to_string(),
        ],
        &config,
        "zcode",
    )
    .unwrap();

    let shown = handle_local_command(
        &["mcp".to_string(), "get".to_string(), "docs".to_string()],
        &config,
        "zcode",
    )
    .unwrap();
    assert!(shown.contains("\"type\": \"sse\""));
    assert!(shown.contains("https://example.com/sse"));
}

#[test]
fn user_mcp_config_path_prefers_xdg() {
    assert_eq!(
        user_mcp_config_path_from(Some("/xdg"), Some("/home/u")).unwrap(),
        PathBuf::from("/xdg/zcode/mcp.json")
    );
    assert_eq!(
        user_mcp_config_path_from(None, Some("/home/u")).unwrap(),
        PathBuf::from("/home/u/.config/zcode/mcp.json")
    );
    assert!(user_mcp_config_path_from(None, None).is_err());
}

#[test]
fn detect_auth_status_prefers_env_key_and_masks_it() {
    let status = detect_auth_status_with(
        |key| (key == "ZCODE_API_KEY").then(|| "sk-zcode-1234567890".to_string()),
        None,
    );
    match status {
        AuthStatus::EnvKey { variable, masked } => {
            assert_eq!(variable, "ZCODE_API_KEY");
            assert!(!masked.contains("1234567890"));
            assert!(masked.contains("7890"));
        }
        other => panic!("expected env key auth, got {other:?}"),
    }
}

#[test]
fn detect_auth_status_finds_credential_file_then_none() {
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(
        detect_auth_status_with(|_| None, Some(temp.path())),
        AuthStatus::None
    );

    let creds = temp.path().join(".zcode").join("credentials.json");
    fs::create_dir_all(creds.parent().unwrap()).unwrap();
    fs::write(&creds, "{}").unwrap();
    assert_eq!(
        detect_auth_status_with(|_| None, Some(temp.path())),
        AuthStatus::CredentialFile(creds)
    );
}

#[test]
fn auth_commands_use_default_or_override() {
    assert_eq!(
        login_command("zcode", None).unwrap(),
        vec!["zcode", "login"]
    );
    assert_eq!(
        logout_command("/opt/zcode", None).unwrap(),
        vec!["/opt/zcode", "logout"]
    );
    assert_eq!(
        login_command("zcode", Some("zcode login --no-browser")).unwrap(),
        vec!["zcode", "login", "--no-browser"]
    );
    assert!(login_command("zcode", Some("  ")).is_err());
}

#[test]
fn mask_secret_hides_middle() {
    assert_eq!(mask_secret("short"), "****");
    let masked = mask_secret("sk-abcdefghijklmnop");
    assert!(masked.starts_with("sk-a"));
    assert!(masked.ends_with("mnop"));
    assert!(!masked.contains("cdefghijkl"));
}

#[test]
fn strip_ansi_removes_color_sequences() {
    assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m plain"), "red plain");
}

#[test]
fn markdown_renders_headings_emphasis_and_code() {
    use zcode_tui::{markdown_lines, MdLineKind, SpanRole};

    let lines = markdown_lines(
        "# Title\n\n**bold** and `code`\n\n```rust\nfn x() {}\n```",
        0,
    );

    assert_eq!(lines[0].kind, MdLineKind::Heading);
    assert_eq!(lines[0].spans[0].text, "Title");

    let body = &lines[1];
    assert_eq!(body.spans[0].role, SpanRole::Strong);
    assert_eq!(body.spans[0].text, "bold");
    assert!(body
        .spans
        .iter()
        .any(|span| span.role == SpanRole::Code && span.text == "code"));

    let code: Vec<_> = lines
        .iter()
        .filter(|line| line.kind == MdLineKind::CodeBlock)
        .collect();
    let joined: Vec<String> = code
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect();
    assert!(joined.iter().any(|line| line.contains("fn x() {}")));
    // Known language means syntect colors at least one span.
    assert!(code
        .iter()
        .flat_map(|line| line.spans.iter())
        .any(|span| span.color.is_some()));
}

#[test]
fn markdown_diff_fences_become_diff_blocks() {
    use zcode_tui::{markdown_lines, MdLineKind};

    let lines = markdown_lines("```diff\n+added\n-removed\n context\n```", 0);
    let blocks: Vec<_> = lines
        .iter()
        .filter(|line| line.kind == MdLineKind::DiffBlock)
        .collect();
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].spans[0].text, "+added");
    assert_eq!(blocks[1].spans[0].text, "-removed");
}

#[test]
fn update_feed_parsing_and_version_compare() {
    use zcode_tui::{is_newer_version, parse_update_feed, parse_update_feed_url};

    let feed_url = parse_update_feed_url(
        "provider: generic\nurl: https://cdn.example.com/update/linux/x64/\n",
    );
    assert_eq!(
        feed_url.as_deref(),
        Some("https://cdn.example.com/update/linux/x64/latest-linux.yml")
    );

    let feed = parse_update_feed(
        "version: 3.2.5\nfiles:\n  - url: ZCode-3.2.5-linux-x64.AppImage\n  - url: ZCode-3.2.5-linux-x64.deb\nreleaseName: Release v3.2.5\n",
    )
    .unwrap();
    assert_eq!(feed.version, "3.2.5");
    assert_eq!(feed.deb_file.as_deref(), Some("ZCode-3.2.5-linux-x64.deb"));
    assert_eq!(feed.release_name.as_deref(), Some("Release v3.2.5"));

    assert!(is_newer_version("3.2.5", "3.2.3"));
    assert!(is_newer_version("3.10.0", "3.9.9"));
    assert!(!is_newer_version("3.2.3", "3.2.3"));
    assert!(!is_newer_version("3.2.3", "3.2.5"));
}

#[test]
fn ide_command_uses_override_and_classifies_local() {
    use zcode_tui::ide_command;

    assert_eq!(
        ide_command(Some("code --new-window"), "/tmp/p").unwrap(),
        vec!["code", "--new-window", "/tmp/p"]
    );
    assert!(ide_command(Some("  "), "/tmp/p").is_err());
    assert_eq!(
        classify_input("/ide src").unwrap(),
        InputAction::Local(vec!["ide".into(), "src".into()])
    );
}

#[test]
fn wrap_display_is_cjk_aware() {
    use zcode_tui::wrap_display;

    // 中文 characters take two display columns each.
    assert_eq!(wrap_display("中文中文中", 4), vec!["中文", "中文", "中"]);
    assert_eq!(wrap_display("ab中文", 4), vec!["ab中", "文"]);
    assert_eq!(wrap_display("plain", 0), vec!["plain"]);
}

#[test]
fn markdown_tables_render_aligned_columns() {
    use zcode_tui::{markdown_lines, SpanRole};

    let lines = markdown_lines("| 名称 | value |\n| --- | --- |\n| alpha | 1 |", 0);
    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>()
        })
        .collect();

    assert_eq!(lines[0].spans[0].role, SpanRole::Strong);
    assert!(rendered[1].starts_with('─'));
    // Cells align by display width: 名称 (4 cols) pads to alpha's 5 cols.
    assert_eq!(rendered[0].trim_end(), "名称   value");
    assert_eq!(rendered[2].trim_end(), "alpha  1");
}

#[test]
fn classify_session_commands_as_local() {
    assert_eq!(
        classify_input("/mode plan").unwrap(),
        InputAction::Local(vec!["mode".into(), "plan".into()])
    );
    assert_eq!(
        classify_input("/resume sess_123").unwrap(),
        InputAction::Local(vec!["resume".into(), "sess_123".into()])
    );
    assert_eq!(
        classify_input("/new").unwrap(),
        InputAction::Local(vec!["new".into()])
    );
}

#[test]
fn markdown_renders_lists_and_wraps() {
    use zcode_tui::{markdown_lines, SpanRole};

    let lines = markdown_lines("- alpha\n- beta", 0);
    assert_eq!(lines[0].spans[0].role, SpanRole::Marker);
    assert_eq!(lines[0].spans[0].text, "• ");
    assert_eq!(lines[0].spans[1].text, "alpha");

    let wrapped = markdown_lines("abcdefghij", 4);
    assert_eq!(wrapped.len(), 3);
    assert_eq!(wrapped[0].spans[0].text, "abcd");
}

#[test]
fn diff_lines_are_classified() {
    use zcode_tui::{diff_line_role, DiffRole};

    assert_eq!(diff_line_role("diff --git a/x b/x"), DiffRole::Meta);
    assert_eq!(diff_line_role("+++ b/src/lib.rs"), DiffRole::Meta);
    assert_eq!(diff_line_role("@@ -1,4 +1,6 @@"), DiffRole::Hunk);
    assert_eq!(diff_line_role("+added"), DiffRole::Add);
    assert_eq!(diff_line_role("-removed"), DiffRole::Remove);
    assert_eq!(diff_line_role(" context"), DiffRole::Context);
}

#[test]
fn classify_diff_as_local_and_build_git_command() {
    use std::path::Path;
    use zcode_tui::git_diff_command;

    assert_eq!(
        classify_input("/diff --staged").unwrap(),
        InputAction::Local(vec!["diff".into(), "--staged".into()])
    );
    assert_eq!(
        git_diff_command(Path::new("/tmp/p"), &["--staged".to_string()]),
        vec![
            "git",
            "-C",
            "/tmp/p",
            "--no-pager",
            "diff",
            "--no-color",
            "--staged",
        ]
    );
}

#[test]
fn stream_events_are_recognized() {
    use zcode_tui::{parse_stream_event, StreamEvent};

    assert_eq!(
        parse_stream_event(r#"{"type":"tool_use","name":"bash","input":{"cmd":"ls"}}"#),
        Some(StreamEvent::ToolUse {
            name: "bash".to_string(),
            detail: r#"{"cmd":"ls"}"#.to_string(),
        })
    );
    assert_eq!(
        parse_stream_event(r#"{"type":"tool_result","content":"ok"}"#),
        Some(StreamEvent::ToolResult {
            detail: "ok".to_string(),
        })
    );
    assert_eq!(
        parse_stream_event(r#"{"type":"text","text":"hello"}"#),
        Some(StreamEvent::Text("hello".to_string()))
    );
    assert_eq!(
        parse_stream_event(r#"{"type":"session_started","id":"s1"}"#),
        Some(StreamEvent::Meta("session_started".to_string()))
    );
    assert_eq!(parse_stream_event("plain text line"), None);
    assert_eq!(parse_stream_event("{not json}"), None);
}
