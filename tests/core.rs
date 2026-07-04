use std::fs;
use std::path::PathBuf;

use zcode_tui::{
    build_prompt_command, build_prompt_command_with_attachments, classify_input,
    command_palette_rows, context_watermark_warn, db_baseline, db_schema_supported,
    detect_auth_status_with, env_is_headless, extract_file_mentions, file_suggestions,
    fold_preview, format_context_watermark, handle_local_command, history_search, latest_reasoning,
    latest_session_for_dir, leader_action_for_key, list_recent_sessions, live_tool_chips,
    load_mcp_config, login_command, logout_command, mask_secret, open_kernel_db_ro, parse_cli_args,
    parse_hex_color, parse_part_data, parse_prompt_summary, parse_ui_config, recent_input_history,
    relative_age, save_mcp_config, slash_suggestions, strip_ansi, user_mcp_config_path_from,
    AppConfig, AuthStatus, InputAction, LeaderAction, McpServer, PartEvent, ToolChipStatus,
    KNOWN_DB_MIGRATIONS,
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
            "--json",
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
            "--json",
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
fn detect_auth_status_env_key_alone_is_only_partial() {
    // Verified against kernel 0.15.0: env keys without the model config
    // file still fail, so they must never read as fully configured.
    let status = detect_auth_status_with(
        |key| (key == "ZCODE_API_KEY").then(|| "sk-zcode-1234567890".to_string()),
        None,
    );
    match status {
        AuthStatus::Partial { evidence } => {
            assert!(evidence.contains("$ZCODE_API_KEY"));
            assert!(!evidence.contains("1234567890"));
            assert!(evidence.contains("7890"));
        }
        other => panic!("expected partial auth, got {other:?}"),
    }
}

#[test]
fn detect_auth_status_config_json_wins_and_carries_env_key() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join(".zcode").join("cli").join("config.json");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "{}").unwrap();

    let status = detect_auth_status_with(
        |key| (key == "ZCODE_API_KEY").then(|| "sk-zcode-1234567890".to_string()),
        Some(temp.path()),
    );
    match status {
        AuthStatus::Configured {
            config_path,
            env_key,
        } => {
            assert_eq!(config_path, config);
            let (variable, masked) = env_key.expect("env key carried along");
            assert_eq!(variable, "ZCODE_API_KEY");
            assert!(!masked.contains("1234567890"));
        }
        other => panic!("expected configured auth, got {other:?}"),
    }
    assert!(detect_auth_status_with(|_| None, Some(temp.path())).is_configured());
}

#[test]
fn detect_auth_status_credential_file_is_partial_then_none() {
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(
        detect_auth_status_with(|_| None, Some(temp.path())),
        AuthStatus::None
    );

    let creds = temp.path().join(".zcode").join("credentials.json");
    fs::create_dir_all(creds.parent().unwrap()).unwrap();
    fs::write(&creds, "{}").unwrap();
    match detect_auth_status_with(|_| None, Some(temp.path())) {
        AuthStatus::Partial { evidence } => {
            assert!(evidence.contains("credentials.json"));
        }
        other => panic!("expected partial auth, got {other:?}"),
    }
}

#[test]
fn auth_commands_use_default_or_override() {
    assert_eq!(
        login_command("zcode", None, false).unwrap(),
        vec!["zcode", "login"]
    );
    assert_eq!(
        logout_command("/opt/zcode", None).unwrap(),
        vec!["/opt/zcode", "logout"]
    );
    // Headless injects --no-browser into the default command only; an
    // explicit override always runs verbatim.
    assert_eq!(
        login_command("zcode", None, true).unwrap(),
        vec!["zcode", "login", "--no-browser"]
    );
    assert_eq!(
        login_command("zcode", Some("zcode login --custom"), true).unwrap(),
        vec!["zcode", "login", "--custom"]
    );
    assert!(login_command("zcode", Some("  "), false).is_err());
}

#[test]
fn env_is_headless_requires_both_displays_absent() {
    assert!(env_is_headless(|_| None));
    assert!(env_is_headless(
        |key| (key == "DISPLAY").then(|| "  ".to_string())
    ));
    assert!(!env_is_headless(
        |key| (key == "DISPLAY").then(|| ":0".to_string())
    ));
    assert!(!env_is_headless(
        |key| (key == "WAYLAND_DISPLAY").then(|| "wayland-0".to_string())
    ));
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

// ---- kernel db consumer -------------------------------------------------

/// Minimal kernel-shaped db for consumer tests: real table names, only the
/// columns the read-only queries touch.
fn fake_kernel_db(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("db.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migration (id TEXT PRIMARY KEY);
         CREATE TABLE session (id TEXT PRIMARY KEY, title TEXT, directory TEXT, time_updated INTEGER);
         CREATE TABLE input_history (id TEXT PRIMARY KEY, text TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT, data TEXT);
         CREATE TABLE tool_usage (id TEXT PRIMARY KEY, session_id TEXT, tool_name TEXT, \
          status TEXT, duration_ms INTEGER, cancelled_by_user INTEGER);",
    )
    .unwrap();
    for migration in KNOWN_DB_MIGRATIONS {
        conn.execute("INSERT INTO schema_migration (id) VALUES (?1)", [migration])
            .unwrap();
    }
    path
}

#[test]
fn db_schema_check_allows_newer_but_rejects_missing_migrations() {
    let temp = tempfile::tempdir().unwrap();
    let path = fake_kernel_db(temp.path());
    let writer = rusqlite::Connection::open(&path).unwrap();

    let ro = open_kernel_db_ro(&path).unwrap();
    assert!(db_schema_supported(&ro));

    // A kernel upgrade appending migrations must not disable the consumer.
    writer
        .execute(
            "INSERT INTO schema_migration (id) VALUES ('0014_future_migration')",
            [],
        )
        .unwrap();
    assert!(db_schema_supported(&ro));

    // Any known id missing means the schema moved under us.
    writer
        .execute(
            "DELETE FROM schema_migration WHERE id = ?1",
            [KNOWN_DB_MIGRATIONS[0]],
        )
        .unwrap();
    assert!(!db_schema_supported(&ro));

    // No table / no file both read as unsupported, never as an error.
    writer.execute("DROP TABLE schema_migration", []).unwrap();
    assert!(!db_schema_supported(&ro));
    assert!(open_kernel_db_ro(&temp.path().join("missing.sqlite")).is_err());
}

#[test]
fn db_session_resolution_and_live_queries() {
    let temp = tempfile::tempdir().unwrap();
    let path = fake_kernel_db(temp.path());
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "INSERT INTO session VALUES ('sess_old', 'old work', '/proj', 100);
             INSERT INTO session VALUES ('sess_new', NULL, '/proj', 200);
             INSERT INTO session VALUES ('sess_other', 'elsewhere', '/elsewhere', 300);",
        )
        .unwrap();

    let ro = open_kernel_db_ro(&path).unwrap();
    assert_eq!(
        latest_session_for_dir(&ro, "/proj"),
        Some("sess_new".to_string())
    );
    assert_eq!(latest_session_for_dir(&ro, "/nowhere"), None);

    // Baseline excludes pre-existing rows; later inserts and in-place
    // status updates are both visible on re-read.
    writer
        .execute_batch(
            "INSERT INTO tool_usage VALUES ('t0', 'sess_new', 'OldTool', 'completed', 5, 0);",
        )
        .unwrap();
    let baseline = db_baseline(&ro);
    writer
        .execute_batch(
            "INSERT INTO tool_usage VALUES ('t1', 'sess_new', 'Read', 'running', NULL, 0);
             INSERT INTO tool_usage VALUES ('t2', 'sess_new', 'Bash', 'completed', 103, 0);
             INSERT INTO tool_usage VALUES ('t3', 'sess_other', 'Grep', 'completed', 9, 0);",
        )
        .unwrap();
    let chips = live_tool_chips(&ro, "sess_new", baseline).unwrap();
    assert_eq!(chips.len(), 2);
    assert_eq!(chips[0].tool, "Read");
    assert_eq!(chips[0].status, ToolChipStatus::Running);
    assert_eq!(chips[1].status, ToolChipStatus::Completed);
    assert_eq!(chips[1].duration_ms, Some(103));

    writer
        .execute(
            "UPDATE tool_usage SET status = 'failed' WHERE id = 't1'",
            [],
        )
        .unwrap();
    let chips = live_tool_chips(&ro, "sess_new", baseline).unwrap();
    assert_eq!(chips[0].status, ToolChipStatus::Failed);

    writer
        .execute_batch(
            "INSERT INTO part VALUES ('p1', 'sess_new', \
              '{\"type\":\"reasoning\",\"text\":\"  \\nScanning the repo\\nmore\"}');
             INSERT INTO part VALUES ('p2', 'sess_new', '{\"type\":\"unknown-future\"}');",
        )
        .unwrap();
    assert_eq!(
        latest_reasoning(&ro, "sess_new", baseline).unwrap(),
        Some("Scanning the repo".to_string())
    );
}

#[test]
fn part_data_parses_real_kernel_samples_and_skips_unknown() {
    // Captured from a real 0.15.0 run (2026-07-04 spike).
    let tool = r#"{"type":"tool","callID":"call_e484","tool":"Bash","state":{"status":"completed","input":{"command":"echo hello"},"output":"hello","title":"Bash"}}"#;
    assert_eq!(
        parse_part_data(tool),
        Some(PartEvent::Tool {
            call_id: "call_e484".to_string(),
            tool: "Bash".to_string(),
            status: ToolChipStatus::Completed,
        })
    );
    assert_eq!(
        parse_part_data(r#"{"type":"text","text":"hi","time":1}"#),
        Some(PartEvent::Text("hi".to_string()))
    );
    assert_eq!(
        parse_part_data(r#"{"type":"step-finish","cost":0,"tokens":{}}"#),
        Some(PartEvent::StepFinish)
    );
    assert_eq!(parse_part_data(r#"{"type":"hologram"}"#), None);
    assert_eq!(parse_part_data("not json at all"), None);
}

// ---- prompt --json summary ------------------------------------------------

#[test]
fn prompt_summary_parses_real_shape_and_falls_back_on_text() {
    // Shape captured from a real 0.15.0 run.
    let raw = r#"{
  "sessionId": "sess_43fd89a8",
  "traceId": "519c3389",
  "turnId": "turn_c8894b36",
  "response": "**Files:**\n- data.txt",
  "usage": { "totalTokens": 17859 },
  "eventCount": 173,
  "projection": { "status": "idle", "contextUsed": 9055, "contextWindow": 200000 }
}"#;
    let summary = parse_prompt_summary(raw).expect("summary parses");
    assert_eq!(summary.response, "**Files:**\n- data.txt");
    assert_eq!(summary.session_id.as_deref(), Some("sess_43fd89a8"));
    assert_eq!(summary.context_used, Some(9055));
    assert_eq!(summary.context_window, Some(200000));
    assert_eq!(summary.total_tokens, Some(17859));

    assert_eq!(parse_prompt_summary("plain text answer"), None);
    assert_eq!(parse_prompt_summary(r#"{"no_response": true}"#), None);
}

#[test]
fn prompt_command_carries_json_flag_exactly_once() {
    let config = AppConfig::default();
    let command = build_prompt_command("zcode", &config, "hello");
    assert_eq!(
        command.iter().filter(|arg| *arg == "--json").count(),
        1,
        "exactly one --json: {command:?}"
    );
    let with_passthrough = AppConfig {
        passthrough: vec!["--json".to_string()],
        ..Default::default()
    };
    let command = build_prompt_command("zcode", &with_passthrough, "hello");
    assert_eq!(command.iter().filter(|arg| *arg == "--json").count(), 1);
}

#[test]
fn context_watermark_formats_and_warns() {
    assert_eq!(format_context_watermark(9055, 200000), "ctx 9k/200k (4%)");
    assert_eq!(format_context_watermark(512, 0), "ctx 512");
    assert!(!context_watermark_warn(9055, 200000));
    assert!(context_watermark_warn(160000, 200000));
    assert!(!context_watermark_warn(1, 0));
}

// ---- session picker / history / folding / ui config -----------------------

#[test]
fn recent_sessions_current_dir_first_with_title_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let path = fake_kernel_db(temp.path());
    let ro = open_kernel_db_ro(&path).unwrap();
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "INSERT INTO session VALUES ('sess_a', 'fix the bug', '/proj', 100);
             INSERT INTO session VALUES ('sess_b', NULL, '/proj/sub', 300);
             INSERT INTO session VALUES ('sess_c', '', '/other', 200);",
        )
        .unwrap();

    let rows = list_recent_sessions(&ro, "/proj", 10).unwrap();
    // Current-directory session first despite being the oldest.
    assert_eq!(rows[0].id, "sess_a");
    assert_eq!(rows[0].title, "fix the bug");
    // Missing/empty titles fall back to the directory tail.
    let by_id = |id: &str| rows.iter().find(|row| row.id == id).unwrap().clone();
    assert_eq!(by_id("sess_b").title, "sub");
    assert_eq!(by_id("sess_c").title, "other");
    // Remaining rows by recency.
    assert_eq!(rows[1].id, "sess_b");
    assert_eq!(rows[2].id, "sess_c");

    assert_eq!(list_recent_sessions(&ro, "/proj", 2).unwrap().len(), 2);
}

#[test]
fn relative_age_buckets() {
    assert_eq!(relative_age(1_000_000, 990_000), "now");
    assert_eq!(relative_age(1_000_000, 1_000_000 - 5 * 60_000), "5m");
    assert_eq!(
        relative_age(1_000_000_000, 1_000_000_000 - 3 * 3_600_000),
        "3h"
    );
    assert_eq!(
        relative_age(1_000_000_000_000, 1_000_000_000_000 - 2 * 86_400_000),
        "2d"
    );
}

#[test]
fn input_history_reads_oldest_first_and_dedups() {
    let temp = tempfile::tempdir().unwrap();
    let path = fake_kernel_db(temp.path());
    let ro = open_kernel_db_ro(&path).unwrap();
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "INSERT INTO input_history VALUES ('i1', 'first');
             INSERT INTO input_history VALUES ('i2', 'second');
             INSERT INTO input_history VALUES ('i3', 'second');
             INSERT INTO input_history VALUES ('i4', '   ');
             INSERT INTO input_history VALUES ('i5', 'third');",
        )
        .unwrap();

    assert_eq!(
        recent_input_history(&ro, 200).unwrap(),
        vec!["first", "second", "third"]
    );
    // The limit applies to the newest entries.
    assert_eq!(recent_input_history(&ro, 2).unwrap(), vec!["third"]);
}

#[test]
fn history_search_is_substring_newest_first() {
    let history: Vec<String> = ["cargo test", "/mcp list", "cargo build", "/mcp list"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        history_search(&history, "mcp", 10),
        vec!["/mcp list".to_string()]
    );
    assert_eq!(
        history_search(&history, "CARGO", 10),
        vec!["cargo build".to_string(), "cargo test".to_string()]
    );
    assert_eq!(history_search(&history, "", 2).len(), 2);
    assert!(history_search(&history, "nothing", 10).is_empty());
}

#[test]
fn fold_preview_thresholds() {
    let long = ["line"; 120].join("\n");
    assert_eq!(fold_preview(&long, 24, 8), Some((8, 112)));
    let short = ["line"; 10].join("\n");
    assert_eq!(fold_preview(&short, 24, 8), None);
    assert_eq!(fold_preview(&long, 24, 200), None);
}

#[test]
fn ui_config_parses_colors_and_mouse_ignoring_junk() {
    let config = parse_ui_config(
        "# comment\n\
         accent = #ff8800\n\
         brand=#B26CC4\n\
         accent = 不是颜色\n\
         unknown_key = #112233\n\
         mouse = off\n\
         mouse = maybe\n\
         no equals sign here\n",
    );
    // A later malformed value must not clobber an earlier good one.
    assert_eq!(config.colors.get("accent"), Some(&(0xff, 0x88, 0x00)));
    assert_eq!(config.colors.get("brand"), Some(&(0xb2, 0x6c, 0xc4)));
    assert!(!config.colors.contains_key("unknown_key"));
    assert_eq!(config.mouse, Some(false));

    assert_eq!(parse_ui_config(""), zcode_tui::UiConfig::default());
    assert_eq!(parse_hex_color("#12345"), None);
    assert_eq!(parse_hex_color("123456"), None);
    assert_eq!(parse_hex_color(" #A1b2C3 "), Some((0xa1, 0xb2, 0xc3)));
}
