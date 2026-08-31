use std::fs;
use std::path::PathBuf;

use zcode_tui::{
    build_prompt_command, build_prompt_command_with_attachments, classify_input, command_catalog,
    command_palette_rows, context_watermark_warn, db_baseline, db_schema_supported,
    detect_auth_status_with, env_is_headless, extract_file_mentions, file_suggestions,
    format_context_watermark, handle_local_command, history_search, latest_assistant_text,
    latest_reasoning, latest_session_for_dir, leader_action_for_key, list_recent_sessions,
    live_tool_chips, load_mcp_config, login_command, logout_command, mask_secret,
    open_kernel_db_ro, pad_display, parse_cli_args, parse_hex_color, parse_part_data,
    parse_prompt_summary, parse_ui_config, path_tail, recent_input_history, relative_age,
    save_mcp_config, save_ui_theme_to, single_line, slash_suggestions, strip_ansi,
    theme_registry::{built_in_theme, theme_name_list, theme_names, BUILT_IN_THEMES},
    tool_result_summary, user_home_dir_from, user_mcp_config_path_from, AppConfig, AuthStatus,
    InputAction, LeaderAction, McpServer, PartEvent, ToolChipStatus, KNOWN_DB_MIGRATIONS,
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
fn parse_args_carries_zcode_336_tool_policy_and_permission_alias() {
    let config = parse_cli_args([
        "tui",
        "--allowed-tools",
        "Read,Glob",
        "Bash(git *)",
        "--disallowedTools",
        "Write",
        "Bash(rm *)",
        "--mode",
        "edit",
        "--permission-mode=default",
    ])
    .unwrap();

    assert_eq!(config.tool_allowlist, vec!["Read", "Glob", "Bash(git *)"]);
    assert_eq!(config.tool_denylist, vec!["Write", "Bash(rm *)"]);
    assert_eq!(config.mode.as_deref(), Some("build"));

    assert!(parse_cli_args(["--allowed-tools", "--mode", "plan"]).is_err());
    assert!(parse_cli_args(["--permission-mode", "anything"]).is_err());
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
fn browser_use_is_explicit_and_preserved_on_classic_prompt() {
    let config = parse_cli_args([
        "--browser-use=headless",
        "--browser-executable",
        "/opt/chrome/chrome",
    ])
    .unwrap();
    assert_eq!(config.browser_use.as_deref(), Some("headless"));
    assert_eq!(
        config.browser_executable.as_deref(),
        Some("/opt/chrome/chrome")
    );
    assert!(config.passthrough.is_empty());
    let command = build_prompt_command("zcode", &config, "browse docs");
    assert_eq!(
        command,
        vec![
            "zcode",
            "--browser-use",
            "headless",
            "--browser-executable",
            "/opt/chrome/chrome",
            "--json",
            "--prompt",
            "browse docs",
        ]
    );
    assert!(parse_cli_args(["--browser-use", "invalid"]).is_err());
    assert!(parse_cli_args(["--browser-executable", "/opt/chrome"]).is_err());
    assert!(parse_cli_args(["--browser-use="]).is_err());
}

#[test]
fn classic_prompt_preserves_tool_policy_rules() {
    let config = parse_cli_args([
        "--allowed-tools=Read,Glob",
        "--disallowed-tools",
        "Bash(git *)",
    ])
    .unwrap();
    let command = build_prompt_command("zcode", &config, "inspect");
    assert_eq!(
        command,
        vec![
            "zcode",
            "--allowed-tools",
            "Read",
            "Glob",
            "--disallowed-tools",
            "Bash(git *)",
            "--json",
            "--prompt",
            "inspect",
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
    assert_eq!(
        classify_input("/agents").unwrap(),
        InputAction::Local(vec!["agents".into()])
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
    assert!(rows.iter().any(|row| row.contains("/usage")));
    assert!(rows.iter().any(|row| row.contains("/theme")));
    assert!(rows.iter().any(|row| row.contains("/agents")));
    assert!(rows.iter().any(|row| row.contains("/update")));
    assert!(rows.iter().any(|row| row.contains("! <cmd>")));
}

#[test]
fn command_catalog_has_unique_commands_and_local_model_route() {
    let mut seen = std::collections::HashSet::new();
    for item in command_catalog() {
        assert!(
            seen.insert(item.command),
            "duplicate command catalog entry: {}",
            item.command
        );
    }

    let model = command_catalog()
        .iter()
        .find(|item| item.command == "/model")
        .expect("/model command");
    assert_eq!(model.route, "local");
    assert!(model.summary.contains("app-server"));
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

    #[cfg(unix)]
    let command = [
        "sh".to_string(),
        "-c".to_string(),
        "seq 1 500; echo tail-marker >&2".to_string(),
    ];
    #[cfg(windows)]
    let command = [
        "powershell.exe".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
        "1..500 | ForEach-Object { Write-Output $_ }; [Console]::Error.WriteLine('tail-marker')"
            .to_string(),
    ];

    let job = spawn_streaming_command(&command).unwrap();
    assert_eq!(job.streams, 2);

    let mut lines = Vec::new();
    let mut eofs = 0;
    let mut finished = false;
    while let Ok(event) = job.receiver.recv_timeout(Duration::from_secs(10)) {
        match event {
            JobEvent::Line { text, stderr } => lines.push((text, stderr)),
            JobEvent::Eof => eofs += 1,
            JobEvent::Finished { success, .. } => {
                assert!(success);
                finished = true;
            }
        }
    }

    assert!(finished);
    assert_eq!(eofs, job.streams);
    assert_eq!(lines.iter().filter(|(l, _)| l.as_str() == "500").count(), 1);
    // Lines carry their origin stream, so structured-stdout consumers (the
    // --prompt --json summary) can keep stderr warnings out of the parse.
    assert!(lines
        .iter()
        .all(|(l, stderr)| (l == "tail-marker") == *stderr));
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
        "# Title\n\n**bold** and `code`\n\n```rust\nfn x() {}\nfn y() {}\n```",
        0,
    );

    assert_eq!(lines[0].kind, MdLineKind::Heading);
    assert_eq!(lines[0].spans[0].text, "Title");

    let body = lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.role == SpanRole::Strong))
        .expect("strong body line");
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
    // Each source line opens with a dim, numeric line-number gutter (the
    // `· rust` language label also uses a Marker span, so filter by digits).
    let gutters: Vec<&str> = code
        .iter()
        .filter_map(|line| line.spans.first())
        .filter(|span| span.role == SpanRole::Marker && span.text.trim().parse::<u32>().is_ok())
        .map(|span| span.text.as_str())
        .collect();
    assert_eq!(gutters, vec!["1 ", "2 "]);
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
    use std::cmp::Ordering;
    use zcode_tui::{
        compare_versions, is_newer_version, parse_update_feed, parse_update_feed_url,
        resolve_update_download_url,
    };

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

    let base = "https://cdn.example.com/update/linux/x64/";
    let official = "https://cdn.example.com/releases/3.7.7/linux-x64/ZCode-3.7.7-linux-x64.deb";
    assert_eq!(
        resolve_update_download_url(base, official).as_deref(),
        Some(official)
    );
    assert_eq!(
        resolve_update_download_url(base, "nested/ZCode-3.2.5-linux-x64.deb").as_deref(),
        Some("https://cdn.example.com/update/linux/x64/ZCode-3.2.5-linux-x64.deb")
    );
    assert!(resolve_update_download_url(base, "ftp://example.com/ZCode.deb").is_none());

    assert!(is_newer_version("3.2.5", "3.2.3"));
    assert!(is_newer_version("3.10.0", "3.9.9"));
    assert!(!is_newer_version("3.2.3", "3.2.3"));
    assert!(!is_newer_version("3.2.3", "3.2.5"));
    assert_eq!(compare_versions("3.3.6-2288", "3.3.4"), Ordering::Greater);
}

#[test]
fn discovers_semver_latest_rootless_kernel_and_honours_precedence() {
    use zcode_tui::{discover_zcode_app_dir, zcode_app_version_from_path};

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let versions = home.join(".local/opt/zcode");
    let make_app = |version: &str| {
        let app = versions.join(version).join("opt/ZCode");
        fs::create_dir_all(app.join("resources/glm")).unwrap();
        fs::write(app.join("resources/glm/zcode.cjs"), "// kernel").unwrap();
        app
    };
    let old = make_app("3.9.9");
    let latest = make_app("3.10.0");
    let missing_system = temp.path().join("missing-system");

    assert_eq!(
        discover_zcode_app_dir(None, Some(&missing_system), Some(&home)),
        Some(latest.clone())
    );
    assert_eq!(
        zcode_app_version_from_path(&latest).as_deref(),
        Some("3.10.0")
    );

    let explicit = make_app("3.1.0");
    assert_eq!(
        discover_zcode_app_dir(Some(&explicit), Some(&missing_system), Some(&home)),
        Some(explicit)
    );

    let system = temp.path().join("opt/ZCode");
    fs::create_dir_all(system.join("resources/glm")).unwrap();
    fs::write(system.join("resources/glm/zcode.cjs"), "// system").unwrap();
    assert_eq!(
        discover_zcode_app_dir(None, Some(&system), Some(&home)),
        Some(system)
    );
    assert!(old.is_dir());
}

#[test]
fn update_feed_selection_falls_back_from_implicit_loopback_only() {
    use zcode_tui::{select_update_feed_url, OFFICIAL_ZCODE_LINUX_UPDATE_FEED};

    let local_package = "provider: generic\nurl: http://localhost:8081\n";
    assert_eq!(
        select_update_feed_url(Some(local_package), None).as_deref(),
        Some(OFFICIAL_ZCODE_LINUX_UPDATE_FEED)
    );
    assert_eq!(
        select_update_feed_url(Some(local_package), Some("http://127.0.0.1:9123/")).as_deref(),
        Some("http://127.0.0.1:9123/latest-linux.yml")
    );
    assert!(select_update_feed_url(Some(local_package), Some("not-a-url")).is_none());
    assert_eq!(
        select_update_feed_url(
            Some("provider: generic\nurl: https://mirror.example/zcode/\n"),
            None
        )
        .as_deref(),
        Some("https://mirror.example/zcode/latest-linux.yml")
    );
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
    assert_eq!(
        classify_input("/theme light").unwrap(),
        InputAction::Local(vec!["theme".into(), "light".into()])
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

    let paragraphs = markdown_lines("first paragraph\n\nsecond paragraph", 80);
    assert!(paragraphs[1].spans.is_empty());
    assert_eq!(paragraphs[2].spans[0].text, "second paragraph");
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
         CREATE TABLE message (id TEXT PRIMARY KEY, data TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT, message_id TEXT, data TEXT);
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
            "INSERT INTO message VALUES ('m_user', '{\"role\":\"user\"}');
             INSERT INTO message VALUES ('m_asst', '{\"role\":\"assistant\"}');
             INSERT INTO part VALUES ('p1', 'sess_new', 'm_asst', \
              '{\"type\":\"reasoning\",\"text\":\"  \\nScanning the repo\\nmore\"}');
             INSERT INTO part VALUES ('p2', 'sess_new', 'm_asst', '{\"type\":\"unknown-future\"}');
             INSERT INTO part VALUES ('p3', 'sess_new', 'm_user', \
              '{\"type\":\"text\",\"text\":\"my prompt echo\"}');
             INSERT INTO part VALUES ('p4', 'sess_new', 'm_asst', \
              '{\"type\":\"text\",\"text\":\"the answer forming\"}');",
        )
        .unwrap();
    assert_eq!(
        latest_reasoning(&ro, "sess_new", baseline).unwrap(),
        Some("Scanning the repo".to_string())
    );
    // Only the assistant text part is returned; the user's echo is excluded.
    assert_eq!(
        latest_assistant_text(&ro, "sess_new", baseline).unwrap(),
        Some("the answer forming".to_string())
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

    // Tool-using turns stream NDJSON (event objects, then the summary last).
    // The parser must skip the event lines and pick the final summary object.
    let ndjson = concat!(
        r#"{"type":"tool_call","toolName":"Read"}"#,
        "\n",
        r#"{"type":"tool_result","content":"..."}"#,
        "\n",
        r#"{"sessionId":"sess_x","response":"done reading","projection":{"contextUsed":42,"contextWindow":200000}}"#,
        "\n",
    );
    let summary = parse_prompt_summary(ndjson).expect("ndjson summary parses");
    assert_eq!(summary.response, "done reading");
    assert_eq!(summary.session_id.as_deref(), Some("sess_x"));
    assert_eq!(summary.context_used, Some(42));
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
    assert_eq!(format_context_watermark(9055, 200000), "ctx 9.1k/200k (4%)");
    assert_eq!(format_context_watermark(512, 0), "ctx 512");
    assert!(!context_watermark_warn(9055, 200000));
    assert!(context_watermark_warn(160000, 200000));
    assert!(!context_watermark_warn(1, 0));
}

// ---- session picker / history / tool presentation / ui config -------------

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
fn successful_internal_tools_use_structured_summaries() {
    assert_eq!(
        tool_result_summary(
            "Read",
            r#"{"file_path":"/tmp/notes.txt"}"#,
            "one\ntwo\nthree\n",
            true,
            Some(8),
        ),
        "Read  notes.txt  · 3 lines  · 8ms  · passed"
    );
    let bash = tool_result_summary(
        "Bash",
        "cargo test",
        "very long successful output",
        true,
        None,
    );
    assert_eq!(bash, "Bash  cargo test  · passed");
    assert!(!bash.contains("very long"));
}

#[test]
fn failed_internal_tools_keep_a_bounded_diagnostic_tail() {
    let output = (1..=7)
        .map(|line| format!("diagnostic {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let summary = tool_result_summary("Bash", "cargo test", &output, false, Some(4200));
    assert!(summary.starts_with("Bash  cargo test  · 4.2s  · failed"));
    assert!(!summary.contains("diagnostic 3"));
    assert!(summary.contains("diagnostic 4"));
    assert!(summary.contains("… 3 more diagnostic lines"));
}

#[test]
fn help_no_longer_advertises_output_folding() {
    let help = zcode_tui::help_text();
    assert!(!help.contains("Ctrl+O"));
    assert!(help.contains("keyboard shortcuts:"));
    assert!(help.contains("Ctrl+J"));
    assert!(help.contains("Ctrl+P"));
    assert!(help.contains("Ctrl+X, then h"));
    assert!(help.contains("PgUp/PgDn"));
    assert!(help.find("keyboard shortcuts:") < help.find("launch options:"));
}

#[test]
fn theme_registry_drives_help_parsing_and_save_validation() {
    let help = zcode_tui::help_text();
    assert!(help.contains(&format!("/theme [list|{}]", theme_name_list("|"))));
    assert_eq!(theme_names().count(), 11);

    for registered in BUILT_IN_THEMES {
        assert_eq!(
            parse_ui_config(&format!("theme = {}", registered.name))
                .theme
                .as_deref(),
            Some(registered.name)
        );
    }
    assert!(parse_ui_config("theme = ultraviolet").theme.is_none());

    let temp = tempfile::tempdir().unwrap();
    let error = save_ui_theme_to(&temp.path().join("config"), "ultraviolet").unwrap_err();
    assert!(error.to_string().contains(&theme_name_list(", ")));
}

#[test]
fn ui_config_parses_colors_and_notify_ignoring_junk() {
    let config = parse_ui_config(
        "# comment\n\
         theme = light\n\
         theme = ultraviolet\n\
         accent = #ff8800\n\
         selection_fg = #f0e442\n\
         accent = 不是颜色\n\
         unknown_key = #112233\n\
         notify = off\n\
         notify = maybe\n\
         no equals sign here\n",
    );
    // A later malformed value must not clobber an earlier good one.
    assert_eq!(config.colors.get("accent"), Some(&(0xff, 0x88, 0x00)));
    assert_eq!(config.colors.get("selection_fg"), Some(&(0xf0, 0xe4, 0x42)));
    assert!(!config.colors.contains_key("unknown_key"));
    assert_eq!(config.notify, Some(false));
    assert_eq!(config.theme.as_deref(), Some("light"));

    assert_eq!(parse_ui_config(""), zcode_tui::UiConfig::default());
    assert_eq!(parse_hex_color("#12345"), None);
    assert_eq!(parse_hex_color("123456"), None);
    assert_eq!(parse_hex_color(" #A1b2C3 "), Some((0xa1, 0xb2, 0xc3)));
}

#[test]
fn ui_theme_persistence_preserves_config_and_crlf() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("nested/config");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "# keep this\r\ntheme = dark\r\naccent = #ff8800\r\ntheme = dark\r\n",
    )
    .unwrap();

    save_ui_theme_to(&path, "light").unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert_eq!(saved.matches("theme = light").count(), 1);
    assert!(saved.contains("# keep this\r\n"));
    assert!(saved.contains("accent = #ff8800\r\n"));
    assert!(!saved.replace("\r\n", "").contains('\n'));
    assert_eq!(parse_ui_config(&saved).theme.as_deref(), Some("light"));
    assert!(save_ui_theme_to(&path, "ultraviolet").is_err());
}

#[test]
fn ui_theme_persistence_accepts_all_named_built_ins() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config");

    for registered in BUILT_IN_THEMES {
        save_ui_theme_to(&path, registered.name).unwrap();
        assert_eq!(
            parse_ui_config(&fs::read_to_string(&path).unwrap())
                .theme
                .as_deref(),
            Some(registered.name)
        );
    }
}

#[test]
fn ui_config_parses_multiple_custom_themes_with_base_fallbacks() {
    let config = parse_ui_config(
        "theme = my-light\n\
         [[custom_themes]]\n\
         name = \"my-light\"\n\
         base = \"light\"\n\
         accent = \"#ff8800\"\n\
         [[custom_themes]]\n\
         name = \"default-base\"\n\
         selection_fg = \"#010203\"\n",
    );

    assert!(config.errors.is_empty());
    assert_eq!(config.theme.as_deref(), Some("my-light"));
    assert_eq!(config.themes.custom_themes().len(), 2);
    let light = config.themes.palette("my-light").unwrap();
    assert_eq!(light.accent, (255, 136, 0));
    assert_eq!(light.text, built_in_theme("light").unwrap().palette.text);
    assert!(light.light);
    let default_base = config.themes.palette("default-base").unwrap();
    assert_eq!(default_base.selection_fg, (1, 2, 3));
    assert_eq!(
        default_base.code_bg,
        built_in_theme("dark").unwrap().palette.code_bg
    );
    assert!(!default_base.light);
}

#[test]
fn ui_config_reports_invalid_custom_themes_and_keeps_valid_entries() {
    let overlong = "a".repeat(33);
    let config = parse_ui_config(&format!(
        "[[custom_themes]]\nname = \"dark\"\n\
         [[custom_themes]]\nname = \"\"\n\
         [[custom_themes]]\nname = \"bad_name\"\n\
         [[custom_themes]]\nname = \"{overlong}\"\n\
         [[custom_themes]]\nname = \"bad-color\"\naccent = \"orange\"\n\
         [[custom_themes]]\nname = \"bad-base\"\nbase = \"ultraviolet\"\n\
         [[custom_themes]]\nname = \"valid-one\"\naccent = \"#112233\"\n\
         [[custom_themes]]\nname = \"valid-one\"\n"
    ));
    let errors = config.errors.join("\n");

    assert!(errors.contains("conflicts with a built-in theme"));
    assert!(errors.contains("name cannot be empty"));
    assert!(errors.contains("invalid custom theme name 'bad_name'"));
    assert!(errors.contains("too long"));
    assert!(errors.contains("invalid accent color"));
    assert!(errors.contains("unknown custom theme base 'ultraviolet'"));
    assert!(errors.contains("duplicate custom theme name 'valid-one'"));
    assert_eq!(config.themes.custom_themes().len(), 1);
    assert_eq!(
        config.themes.palette("valid-one").unwrap().accent,
        (17, 34, 51)
    );
}

#[test]
fn dynamic_registry_drives_custom_help_listing_persistence_and_restore() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config");
    let custom_section = "[[custom_themes]]\nname = \"my-theme\"\nbase = \"light\"\naccent = \"#ff8800\"\nselection_fg = \"#ffffff\"\n";
    fs::write(&path, custom_section).unwrap();

    let initial = parse_ui_config(custom_section);
    let help = zcode_tui::help_text_with_registry(&initial.themes);
    assert!(help.contains("|accessible|my-theme]"));
    assert!(initial
        .themes
        .display_list(", ")
        .contains("my-theme (custom)"));
    assert!(initial.themes.name_list(", ").ends_with(", my-theme"));

    save_ui_theme_to(&path, "my-theme").unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.starts_with("theme = my-theme\n[[custom_themes]]"));
    assert!(saved.contains(custom_section));
    let restored = parse_ui_config(&saved);
    assert_eq!(restored.theme.as_deref(), Some("my-theme"));
    assert_eq!(
        restored.themes.palette("my-theme").unwrap().accent,
        (255, 136, 0)
    );
}

#[test]
fn invalid_custom_theme_cannot_be_selected_and_does_not_rewrite_config() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config");
    let original = "notify = off\n[[custom_themes]]\nname = \"broken\"\naccent = \"orange\"\n";
    fs::write(&path, original).unwrap();

    let error = save_ui_theme_to(&path, "broken").unwrap_err();
    assert!(error.to_string().contains("unknown theme broken"));
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn windows_home_paths_and_session_text_have_safe_fallbacks() {
    use std::ffi::OsStr;

    assert_eq!(
        user_home_dir_from(None, Some(OsStr::new(r"C:\Users\Ada"))),
        Some(std::path::PathBuf::from(r"C:\Users\Ada"))
    );
    assert_eq!(path_tail(r"C:\Users\Ada\project"), Some("project"));
    assert_eq!(single_line("first\r\nsecond"), "first second");
    assert_eq!(
        unicode_width::UnicodeWidthStr::width(pad_display("中文标题", 7).as_str()),
        7
    );
    assert_eq!(pad_display("anything", 0), "");
}

// ---- app-server protocol client -----------------------------------------

#[test]
fn app_request_envelope_has_no_jsonrpc() {
    use zcode_tui::{app_create_params, encode_app_request};
    let line = encode_app_request(1, "session/create", app_create_params("/proj"));
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["id"], 1);
    assert_eq!(v["method"], "session/create");
    assert_eq!(v["params"]["workspace"]["workspaceKey"], "/proj");
    assert_eq!(v["params"]["workspace"]["workspacePath"], "/proj");
    // The kernel rejects a jsonrpc field; we must never emit one.
    assert!(v.get("jsonrpc").is_none());
    assert!(!line.contains('\n'));
}

#[test]
fn subscribe_params_request_continuous_delivery() {
    use zcode_tui::{app_subscribe_params, APP_SERVER_DELIVERY_KIND};
    let p = app_subscribe_params("sess_1");
    assert_eq!(p["sessionId"], "sess_1");
    assert_eq!(p["deliveryKind"], APP_SERVER_DELIVERY_KIND);
    assert_eq!(p["deliveryKind"], "desktop-continuous");
    assert_eq!(p["includeSnapshot"], true);
}

#[test]
fn decode_dispatches_response_event_and_state() {
    use zcode_tui::{decode_app_message, AppServerEvent, AppServerMessage};
    // Response with result.
    let m = decode_app_message(r#"{"id":1,"result":{"session":{"sessionId":"sess_x"}}}"#).unwrap();
    match m {
        AppServerMessage::Response { id, result, error } => {
            assert_eq!(id, 1);
            assert!(error.is_none());
            assert_eq!(
                zcode_tui::app_session_id_from_result(&result.unwrap()).as_deref(),
                Some("sess_x")
            );
        }
        other => panic!("expected response, got {other:?}"),
    }
    // Response with error.
    let m = decode_app_message(r#"{"id":2,"error":{"code":-32601,"message":"Method not found"}}"#)
        .unwrap();
    assert!(matches!(
        m,
        AppServerMessage::Response { error: Some(e), .. } if e.contains("Method not found")
    ));
    // session/event carrying a text delta.
    let raw = r#"{"method":"session/event","params":{"deliveryKind":"desktop-continuous","payload":{"assistantMessageId":"msg_1","delta":"1","done":false,"kind":"text_delta"}}}"#;
    assert_eq!(
        decode_app_message(raw),
        Some(AppServerMessage::Event(AppServerEvent {
            kind: "text_delta".to_string(),
            delta: "1".to_string(),
            done: false,
            ..Default::default()
        }))
    );
    // state.updated is recognized as its own kind.
    let m =
        decode_app_message(r#"{"method":"state.updated","params":{"patch":{"status":"running"}}}"#)
            .unwrap();
    assert!(matches!(m, AppServerMessage::StateUpdated(_)));
    // Unknown method -> Other; garbage -> None (skip).
    assert_eq!(
        decode_app_message(r#"{"method":"whatever","params":{}}"#),
        Some(AppServerMessage::Other)
    );
    assert_eq!(decode_app_message("not json"), None);
}

#[test]
fn decode_recognizes_server_requests_with_string_ids() {
    use zcode_tui::{decode_app_message, AppServerMessage};
    // Server→client request: method AND id together, STRING envelope id
    // (kernel sends "server-1", "server-2", …). Was silently dropped before —
    // which hung plan-mode turns until the 600s backstop.
    let raw = r#"{"id":"server-1","method":"interaction/requestUserInput","params":{"requestId":"perm_1"}}"#;
    match decode_app_message(raw).unwrap() {
        AppServerMessage::ServerRequest { id, method, params } => {
            assert_eq!(id, serde_json::json!("server-1"));
            assert_eq!(method, "interaction/requestUserInput");
            assert_eq!(params["requestId"], "perm_1");
        }
        other => panic!("expected server request, got {other:?}"),
    }
    // Numeric-id server requests dispatch the same way (id kept as raw JSON).
    let raw = r#"{"id":7,"method":"interaction/requestUserInput","params":{}}"#;
    assert!(matches!(
        decode_app_message(raw).unwrap(),
        AppServerMessage::ServerRequest { id, .. } if id == serde_json::json!(7)
    ));
    // Plain numeric-id responses are unaffected by the new branch.
    assert!(matches!(
        decode_app_message(r#"{"id":3,"result":{"accepted":true}}"#).unwrap(),
        AppServerMessage::Response { id: 3, .. }
    ));
}

#[test]
fn interaction_request_parses_pinned_payload_and_tolerates_gaps() {
    use zcode_tui::{parse_interaction_request, INTERACTION_METHOD};
    // The exact payload shape captured live 2026-07-07 (kernel 0.15.0).
    let params = serde_json::json!({
        "input": {"plan": "Create the file `x.txt` with `hello`."},
        "prompt": "Tool ExitPlanMode requires user interaction",
        "questions": [{
            "header": "Plan",
            "options": [{
                "description": "Exit plan mode and start implementation.",
                "label": "Approve",
                "value": "approve"
            }],
            "question": "Review this implementation plan."
        }],
        "requestId": "perm_3357106e",
        "schema": {"interaction": "plan_approval", "toolName": "ExitPlanMode"},
        "sessionId": "sess_1",
        "toolCallId": "call_2",
        "toolName": "ExitPlanMode",
        "turnId": "turn_3"
    });
    let request = parse_interaction_request(INTERACTION_METHOD, &params).unwrap();
    assert_eq!(request.request_id, "perm_3357106e");
    assert_eq!(request.tool_name, "ExitPlanMode");
    assert_eq!(request.interaction, "plan_approval");
    assert_eq!(
        request.plan.as_deref(),
        Some("Create the file `x.txt` with `hello`.")
    );
    assert_eq!(request.questions.len(), 1);
    let question = &request.questions[0];
    assert_eq!(question.header, "Plan");
    assert_eq!(question.options[0].value, "approve");
    assert_eq!(question.options[0].label, "Approve");
    // No protocol-level decline on plan approvals.
    assert!(request.deny_index.is_none());
    // Missing requestId or questions/options -> None (leave unanswered; the
    // kernel's retry keeps it alive for a client that understands it). An
    // unknown method is never parsed.
    let m = INTERACTION_METHOD;
    assert!(parse_interaction_request(m, &serde_json::json!({"questions": []})).is_none());
    assert!(
        parse_interaction_request(m, &serde_json::json!({"requestId": "p", "questions": []}))
            .is_none()
    );
    assert!(parse_interaction_request(
        m,
        &serde_json::json!({"requestId": "p", "questions": [{"header": "H", "options": []}]})
    )
    .is_none());
    assert!(parse_interaction_request("interaction/other", &params).is_none());
}

#[test]
fn interaction_reply_echoes_envelope_id_verbatim() {
    use zcode_tui::{encode_interaction_reply, parse_interaction_request, INTERACTION_METHOD};
    let params = serde_json::json!({
        "prompt": "Tool ExitPlanMode requires user interaction",
        "questions": [{
            "header": "Plan",
            "options": [{"label": "Approve", "value": "approve"}],
            "question": "Review this implementation plan."
        }],
        "requestId": "perm_9",
        "toolName": "ExitPlanMode"
    });
    let request = parse_interaction_request(INTERACTION_METHOD, &params).unwrap();
    let line = encode_interaction_reply(&serde_json::json!("server-3"), &request, 0).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    // The kernel keys the reply on its own envelope id — echoed as a STRING.
    assert_eq!(v["id"], "server-3");
    assert_eq!(v["result"]["requestId"], "perm_9");
    assert_eq!(v["result"]["answers"]["Plan"], "approve");
    assert!(!line.contains('\n'));
    // Out-of-bounds selection -> None (never sends a malformed reply).
    assert!(encode_interaction_reply(&serde_json::json!("server-3"), &request, 5).is_none());
}

#[test]
fn runtime_preferences_reply_matches_kernel_0163_schema() {
    use zcode_tui::{encode_runtime_preferences_reply, RUNTIME_PREFERENCES_METHOD};

    let line = encode_runtime_preferences_reply(
        &serde_json::json!("server-1"),
        RUNTIME_PREFERENCES_METHOD,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["id"], "server-1");
    assert_eq!(value["result"].as_object().unwrap().len(), 4);
    assert_eq!(value["result"]["nativeSearchEnhancementsEnabled"], true);
    assert_eq!(value["result"]["memoryEnabled"], false);
    assert_eq!(
        value["result"]["askUserQuestionAutoResolutionEnabled"],
        true
    );
    assert_eq!(
        value["result"]["modelContextBudgetStrategy"],
        "preflight-v1"
    );
    assert!(value["result"].get("integratedTerminalShell").is_none());
    assert!(!line.contains('\n'));

    let numeric =
        encode_runtime_preferences_reply(&serde_json::json!(7), RUNTIME_PREFERENCES_METHOD)
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&numeric).unwrap()["id"],
        7
    );
    assert!(encode_runtime_preferences_reply(
        &serde_json::json!("server-2"),
        "interaction/requestUserInput"
    )
    .is_none());
}

#[test]
fn official_mcp_auth_request_matches_kernel_0165_safe_fallback() {
    use zcode_tui::{
        decode_app_message, encode_official_mcp_auth_headers_reply, AppServerMessage,
        OFFICIAL_MCP_AUTH_HEADERS_METHOD,
    };

    let raw = include_str!("fixtures/zcode-0.16.5-official-mcp-auth-request.json");
    let (id, method, params) = match decode_app_message(raw).unwrap() {
        AppServerMessage::ServerRequest { id, method, params } => (id, method, params),
        other => panic!("expected server request, got {other:?}"),
    };
    assert_eq!(method, OFFICIAL_MCP_AUTH_HEADERS_METHOD);
    assert_eq!(params["requestId"], "official-mcp-auth:1");
    assert_eq!(params["mcpKey"], "official-docs");
    assert_eq!(params["pluginId"], "zcode-official");
    assert_eq!(params["targetOrigin"], "https://api.z.ai");
    assert_eq!(params["workspace"]["workspaceKey"], "/repo");

    let line = encode_official_mcp_auth_headers_reply(&id, &method).unwrap();
    let reply: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(reply["id"], "server-2");
    assert_eq!(
        reply["result"],
        serde_json::json!({
            "ok": false,
            "reason": "official_auth_unavailable"
        })
    );
    assert_eq!(reply["result"].as_object().unwrap().len(), 2);
    assert!(!line.contains('\n'));
    assert!(encode_official_mcp_auth_headers_reply(
        &serde_json::json!(9),
        "interaction/requestPermission"
    )
    .is_none());
}

#[test]
fn permission_request_parses_options_and_replies_response_verbatim() {
    use zcode_tui::{encode_interaction_reply, parse_interaction_request, PERMISSION_METHOD};
    // Shape captured live 2026-07-07: a build-mode Write triggers
    // interaction/requestPermission with flat options carrying ready-made
    // response objects; the kernel's reply schema is STRICT — the result must
    // be the chosen option's `response` verbatim, nothing added.
    let params = serde_json::json!({
        "input": {"file_path": "/tmp/w.txt", "content": "hi"},
        "reason": "Tool has side effects and requires approval",
        "requestId": "perm_6ca1a00c",
        "riskLevel": "medium",
        "sessionId": "sess_1",
        "options": [
            {"kind": "allow_once", "name": "Allow once", "optionId": "allow_once",
             "response": {"decision": "allow", "reason": "Approved once"}},
            {"description": "Do not ask again for matching requests in this project",
             "kind": "allow_always", "name": "Always allow in this project",
             "optionId": "allow_project",
             "response": {"decision": "allow", "permissionUpdates": [], "reason": "ok"}},
            {"kind": "deny", "name": "Deny", "optionId": "deny",
             "response": {"decision": "deny", "reason": "Denied"}}
        ],
        "toolCallId": "call_d798",
        "toolName": "Write",
        "turnId": "turn_1096"
    });
    let request = parse_interaction_request(PERMISSION_METHOD, &params).unwrap();
    assert_eq!(request.request_id, "perm_6ca1a00c");
    assert_eq!(request.tool_name, "Write");
    assert_eq!(request.interaction, "permission");
    assert_eq!(request.questions.len(), 1);
    let question = &request.questions[0];
    assert_eq!(question.options.len(), 3);
    assert_eq!(question.options[0].label, "Allow once");
    // The Write target shows in the condensed question line.
    assert!(question.question.contains("Write"));
    assert!(question.question.contains("w.txt"));
    // Esc answers the protocol-level deny option.
    assert_eq!(request.deny_index, Some(2));
    // Approve reply: result == the option's response object, nothing more.
    let line = encode_interaction_reply(&serde_json::json!("server-1"), &request, 0).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["id"], "server-1");
    assert_eq!(
        v["result"],
        serde_json::json!({"decision": "allow", "reason": "Approved once"})
    );
    assert!(v["result"].get("requestId").is_none());
    // Deny reply mirrors the deny option's response.
    let line = encode_interaction_reply(&serde_json::json!("server-2"), &request, 2).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["result"]["decision"], "deny");
}

#[test]
fn session_control_params_match_pinned_schemas() {
    use zcode_tui::{
        app_compact_params, app_set_mode_params, app_set_model_params, app_set_thought_params,
        app_steer_params,
    };
    let p = app_set_mode_params("sess_1", "plan");
    assert_eq!(
        p,
        serde_json::json!({"sessionId": "sess_1", "mode": "plan"})
    );
    let model_ref = serde_json::json!({"modelId": "glm-5.1", "providerId": "bigmodel"});
    let p = app_set_model_params("sess_1", &model_ref);
    assert_eq!(p["sessionId"], "sess_1");
    assert_eq!(p["model"], model_ref);
    let p = app_set_thought_params("sess_1", "disabled");
    assert_eq!(p["sessionId"], "sess_1");
    assert_eq!(p["thoughtLevel"], "disabled");
    assert_eq!(
        app_compact_params("sess_1"),
        serde_json::json!({"sessionId": "sess_1"})
    );
    // steer is shaped exactly like send.
    assert_eq!(
        app_steer_params("sess_1", "focus on tests"),
        serde_json::json!({"sessionId": "sess_1", "content": "focus on tests"})
    );
}

#[test]
fn session_lifecycle_params_and_list_parsing() {
    use zcode_tui::{
        app_create_params, app_resume_params, app_session_read_params,
        app_update_provider_registry_params, app_usage_params, build_runtime_model,
        build_runtime_model_with_desktop, parse_session_list, runtime_model_controls,
        usage_stats_params, with_runtime_model,
    };
    assert_eq!(
        app_resume_params("sess_1", None),
        serde_json::json!({"sessionId": "sess_1"})
    );
    // Resume must be able to carry the runtimeModel that revives the model
    // runtime (resume alone leaves the session RUNTIME_MODEL_UNAVAILABLE).
    let config = r#"{
        "provider": {"bigmodel": {"kind": "anthropic", "name": "Bigmodel Coding Plan",
            "options": {"baseURL": "https://open.bigmodel.cn/api/anthropic", "apiKey": "dummy-api-key-for-test"},
            "models": {"glm-5.1": {"name": "GLM-5.1"}, "glm-4.7": {"name": "GLM-4.7"}}}},
        "model": {"main": "bigmodel/glm-5.1", "lite": "bigmodel/glm-4.7"}
    }"#;
    let runtime = build_runtime_model(config, 1234).unwrap();
    assert_eq!(runtime["model"]["providerId"], "bigmodel");
    assert_eq!(runtime["model"]["modelId"], "glm-5.1");
    assert_eq!(runtime["generatedAt"], 1234);
    let provider = &runtime["provider"];
    assert_eq!(provider["kind"], "anthropic");
    // apiKey is the kernel's credential union: inline carries the value.
    assert_eq!(provider["apiKey"]["source"], "inline");
    assert_eq!(provider["apiKey"]["value"], "dummy-api-key-for-test");
    assert_eq!(
        provider["baseURL"],
        "https://open.bigmodel.cn/api/anthropic"
    );
    assert_eq!(provider["models"].as_array().unwrap().len(), 2);
    let desktop = r#"{
        "provider": {"builtin:bigmodel-coding-plan": {
            "name": "BigModel - Coding Plan", "kind": "anthropic", "enabled": true,
            "models": {
                "GLM-5.3-Flash": {
                    "reasoning": {"enabled": true, "variants": ["low", "max", "high"], "defaultVariant": "max"},
                    "limit": {"context": 1000000, "output": 128000},
                    "modalities": {"input": ["text"], "output": ["text"]}
                },
                "GLM-5.1": {"limit": {"context": 200000, "output": 64000}}
            }
        }}
    }"#;
    let enriched = build_runtime_model_with_desktop(config, Some(desktop), 1236).unwrap();
    let models = enriched["provider"]["models"].as_array().unwrap();
    assert_eq!(
        models.len(),
        3,
        "Desktop merge must preserve CLI-only models"
    );
    let flash = models
        .iter()
        .find(|model| model["modelId"] == "glm-5.3-flash")
        .expect("Flash model merged into the CLI provider");
    assert_eq!(flash["label"], "GLM-5.3-Flash");
    assert_eq!(flash["contextWindow"], 1_000_000);
    assert_eq!(flash["maxOutputTokens"], 128_000);
    assert_eq!(flash["reasoning"]["enabled"], true);
    assert_eq!(flash["reasoning"]["defaultLevel"], "max");
    assert_eq!(flash["reasoning"]["levels"].as_array().unwrap().len(), 3);
    let (provider_id, controls) = runtime_model_controls(&enriched).unwrap();
    assert_eq!(provider_id, "bigmodel");
    let flash_choice = controls
        .models
        .iter()
        .find(|choice| choice.reference["modelId"] == "glm-5.3-flash")
        .unwrap();
    assert_eq!(flash_choice.label, "GLM-5.3-Flash");
    assert_eq!(flash_choice.context_window, Some(1_000_000));
    assert_eq!(flash_choice.reference["providerId"], "bigmodel");
    let create = with_runtime_model(app_create_params("/proj"), Some(&enriched));
    assert_eq!(create["runtimeModel"]["provider"]["providerId"], "bigmodel");
    assert!(create["runtimeModel"]["provider"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["modelId"] == "glm-5.3-flash"));
    let registry = app_update_provider_registry_params("/proj", &enriched).unwrap();
    assert_eq!(registry["workspace"]["workspaceKey"], "/proj");
    assert_eq!(
        registry["registry"]["providers"].as_array().unwrap().len(),
        1
    );
    assert!(registry["registry"]["providers"][0]["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["modelId"] == "glm-5.3-flash"));
    assert_eq!(registry["includeWorkspaceState"], true);
    let malformed = build_runtime_model_with_desktop(config, Some("not json"), 1237).unwrap();
    assert_eq!(malformed["provider"]["models"].as_array().unwrap().len(), 2);
    // Some ZCode configs store the selected model directly at the root.
    let mut root_model_config: serde_json::Value = serde_json::from_str(config).unwrap();
    root_model_config["model"] = serde_json::json!("bigmodel/glm-4.7");
    let root_runtime = build_runtime_model(&root_model_config.to_string(), 1235).unwrap();
    assert_eq!(root_runtime["model"]["modelId"], "glm-4.7");
    let with = app_resume_params("sess_1", Some(&runtime));
    assert_eq!(with["runtimeModel"]["revision"], "zcode-tui-resume");
    // Unknown layouts -> None (bare resume + create fallback), never panic.
    assert!(build_runtime_model("{}", 1).is_none());
    assert!(build_runtime_model("not json", 1).is_none());
    assert_eq!(
        app_usage_params("sess_1"),
        serde_json::json!({"sessionId": "sess_1"})
    );
    assert_eq!(
        app_session_read_params("sess_1"),
        serde_json::json!({"sessionId": "sess_1", "messageLimit": 1})
    );
    assert_eq!(usage_stats_params("7d"), serde_json::json!({"range": "7d"}));
    // session/list result shape captured live 2026-07-07 (kernel 0.15.0).
    let result = serde_json::json!({"sessions": [
        {"sessionId": "sess_old", "title": "Other project", "status": "idle",
         "updatedAt": 100, "workspace": {"workspacePath": "/elsewhere"}},
        {"sessionId": "sess_here", "title": "Create w.txt file", "status": "idle",
         "updatedAt": 50, "workspace": {"workspacePath": "/proj"}},
        {"sessionId": "sess_run", "title": "Busy one", "status": "running",
         "updatedAt": 200, "workspace": {"workspacePath": "/elsewhere"}},
        {"sessionId": "sess_untitled", "status": "idle",
         "updatedAt": 10, "workspace": {"workspacePath": "/elsewhere/deep/dir"}},
        {"sessionId": "sess_windows", "status": "idle",
         "updatedAt": 5, "workspace": {"workspacePath": "C:\\Users\\Ada\\project"}}
    ]});
    let rows = parse_session_list(&result, "/proj");
    // Current-cwd session first, then by recency; running sessions marked.
    assert_eq!(rows[0].id, "sess_here");
    assert_eq!(rows[1].id, "sess_run");
    assert!(rows[1].title.contains("running"));
    assert_eq!(rows[2].id, "sess_old");
    // Missing title falls back to the directory tail.
    assert_eq!(rows[3].title, "dir");
    assert_eq!(rows[4].title, "project");
    // Absent/foreign shapes -> empty, never panic.
    assert!(parse_session_list(&serde_json::json!({}), "/proj").is_empty());
}

#[test]
fn steer_result_union_is_classified() {
    use zcode_tui::{parse_steer_result, SteerOutcome};
    // Kernel FKr union, pinned 2026-07-07: an OK envelope can still carry a
    // rejection — result.kind decides whether the input actually landed.
    let queued = serde_json::json!({"kind": "queued", "pendingInputId": "in_1",
                                    "queueLength": 1, "turnId": "turn_1"});
    assert_eq!(parse_steer_result(&queued), SteerOutcome::Queued);
    let rejected = serde_json::json!({"kind": "rejected", "reason": "turn_not_steerable"});
    assert_eq!(
        parse_steer_result(&rejected),
        SteerOutcome::Rejected("turn_not_steerable".to_string())
    );
    // Unknown shapes assume queued-ish (never double-submit).
    assert_eq!(
        parse_steer_result(&serde_json::json!({"accepted": true})),
        SteerOutcome::Unknown
    );
}

#[test]
fn kernel_slash_commands_merge_into_suggestions() {
    use zcode_tui::{parse_kernel_slash_commands, slash_suggestions_merged};
    // create/resume result shape captured live (slashCommands[]).
    let result = serde_json::json!({"slashCommands": [
        {"name": "goal", "description": "Show or set the current session goal.",
         "inputHint": "/goal [pause|resume|clear]", "source": "builtin"},
        {"name": "review", "description": "Review code changes.",
         "inputHint": "/review [target]", "source": "builtin"}
    ]});
    let kernel = parse_kernel_slash_commands(&result);
    assert_eq!(kernel.len(), 2);
    // /goal exists locally -> kernel duplicate dropped; /review is new.
    let entries = slash_suggestions_merged("/re", 20, &kernel);
    assert!(entries.iter().any(|e| e.command.starts_with("/review")));
    let goal_entries = slash_suggestions_merged("/goal", 20, &kernel);
    let goal_count = goal_entries
        .iter()
        .filter(|e| {
            e.command == "/goal"
                || e.command.starts_with("/goal ")
                || e.command.starts_with("/goal\u{a0}")
        })
        .filter(|e| !e.command.contains("replace"))
        .count();
    assert_eq!(goal_count, 1, "local /goal wins, kernel duplicate dropped");
    // Kernel entry displays its inputHint and routes to zcode.
    let review = entries
        .iter()
        .find(|e| e.command.starts_with("/review"))
        .unwrap();
    assert_eq!(review.command, "/review [target]");
    assert_eq!(review.route, "zcode");
}

#[test]
fn todos_parse_from_result_and_patch() {
    use zcode_tui::parse_todos;
    let result = serde_json::json!({"todos": [
        {"content": "write tests", "status": "completed"},
        {"content": "ship it", "status": "pending"}
    ]});
    let todos = parse_todos(&result).unwrap();
    assert_eq!(todos.len(), 2);
    assert!(todos[0].done);
    assert!(!todos[1].done);
    // state push carries todos under the patch; empty array = explicit clear.
    let push = serde_json::json!({"patch": {"todos": []}});
    assert_eq!(parse_todos(&push).unwrap().len(), 0);
    // No todos key at all -> None (caller keeps previous list).
    assert!(parse_todos(&serde_json::json!({"patch": {"status": "running"}})).is_none());
}

#[test]
fn update_feed_extracts_deb_sha512() {
    use zcode_tui::parse_update_feed;
    // Real 3.2.5 feed structure: files[] entries each carry url+sha512+size;
    // the deb's sha512 must come from ITS entry, not the AppImage's.
    let yaml = "version: 3.2.5\nfiles:\n  - url: ZCode-3.2.5-linux-x64.AppImage\n    sha512: APPIMAGEHASH==\n    size: 153793621\n  - url: ZCode-3.2.5-linux-x64.deb\n    sha512: DEBHASH==\n    size: 113714472\npath: ZCode-3.2.5-linux-x64.AppImage\nsha512: APPIMAGEHASH==\nreleaseName: Release v3.2.5\n";
    let feed = parse_update_feed(yaml).unwrap();
    assert_eq!(feed.version, "3.2.5");
    assert_eq!(feed.deb_file.as_deref(), Some("ZCode-3.2.5-linux-x64.deb"));
    assert_eq!(feed.deb_sha512.as_deref(), Some("DEBHASH=="));
    // A feed without a deb entry parses with both None.
    let feed = parse_update_feed("version: 9.9.9\npath: x.AppImage\n").unwrap();
    assert!(feed.deb_file.is_none());
    assert!(feed.deb_sha512.is_none());
}

#[test]
fn state_controls_extracted_from_mode_changed_patch() {
    use zcode_tui::app_state_controls;
    // Shape captured live from a `reason:"mode_changed"` push (kernel 0.15.0).
    let params = serde_json::json!({
        "reason": "mode_changed",
        "patch": {
            "mode": {"current": "plan"},
            "model": {
                "available": [{
                    "contextWindow": 200000,
                    "label": "glm-5.1",
                    "providerLabel": "BigModel",
                    "ref": {"modelId": "glm-5.1", "providerId": "bigmodel"}
                }],
                "current": {"modelId": "glm-5.1", "providerId": "bigmodel"}
            },
            "permission": {"mode": "plan"},
            "thoughtLevel": {
                "available": [
                    {"label": "enabled", "value": "enabled"},
                    {"label": "disabled", "value": "disabled"}
                ],
                "current": "enabled",
                "enabled": true
            }
        }
    });
    let controls = app_state_controls(&params).unwrap();
    assert_eq!(controls.mode.as_deref(), Some("plan"));
    assert_eq!(controls.models.len(), 1);
    assert_eq!(controls.models[0].label, "glm-5.1");
    assert_eq!(controls.models[0].provider, "BigModel");
    assert_eq!(controls.models[0].context_window, Some(200_000));
    assert_eq!(controls.models[0].reference["providerId"], "bigmodel");
    assert_eq!(controls.model_provider.as_deref(), Some("bigmodel"));
    assert_eq!(controls.model_current.as_deref(), Some("glm-5.1"));
    assert_eq!(controls.thought_levels, vec!["enabled", "disabled"]);
    assert_eq!(controls.thought_current.as_deref(), Some("enabled"));
    // A patch without control keys -> None (nothing to merge).
    assert!(app_state_controls(&serde_json::json!({"patch": {"status": "running"}})).is_none());
    assert!(app_state_controls(&serde_json::json!({"reason": "x"})).is_none());
}

#[test]
fn session_controls_prefer_complete_result_settings() {
    use zcode_tui::app_session_controls;
    let model = |id: &str| {
        serde_json::json!({
            "label": id,
            "providerLabel": "BigModel",
            "ref": {"modelId": id, "providerId": "bigmodel"}
        })
    };
    let result = serde_json::json!({
        "settings": {
            "model": {
                "available": [model("glm-5.1"), model("glm-5.3")],
                "current": {"modelId": "glm-5.1", "providerId": "bigmodel"}
            }
        },
        "snapshot": {
            "settings": {
                "model": {
                    "available": [model("glm-5.1")],
                    "current": {"modelId": "glm-5.1", "providerId": "bigmodel"}
                }
            }
        }
    });
    let controls = app_session_controls(&result).unwrap();
    assert_eq!(controls.models.len(), 2);
    assert_eq!(controls.model_provider.as_deref(), Some("bigmodel"));
    assert_eq!(controls.models[1].reference["modelId"], "glm-5.3");

    let fallback = serde_json::json!({"snapshot": result["snapshot"].clone()});
    assert_eq!(app_session_controls(&fallback).unwrap().models.len(), 1);
}

#[test]
fn workspace_model_catalog_uses_only_the_active_provider() {
    use zcode_tui::{app_workspace_model_controls, app_workspace_read_params};
    let params = app_workspace_read_params("/tmp/project");
    assert_eq!(params["workspace"]["workspaceKey"], "/tmp/project");
    assert_eq!(params["workspace"]["workspacePath"], "/tmp/project");

    let model = |provider_id: &str, provider_label: &str, model_id: &str| {
        serde_json::json!({
            "label": model_id,
            "providerLabel": provider_label,
            "ref": {"modelId": model_id, "providerId": provider_id}
        })
    };
    let result = serde_json::json!({
        "settings": {
            "model": {
                "available": [
                    model("zai", "Z.AI Coding Plan", "glm-5.1"),
                    model("bigmodel", "BigModel Coding Plan", "glm-5.3"),
                    model("bigmodel", "BigModel Coding Plan", "glm-4.7")
                ],
                "current": {"modelId": "glm-5.3", "providerId": "bigmodel"}
            }
        },
        "modelCatalog": {
            "providers": []
        }
    });
    let (provider_id, controls) = app_workspace_model_controls(&result).unwrap();
    assert_eq!(provider_id, "bigmodel");
    assert_eq!(controls.model_provider.as_deref(), Some("bigmodel"));
    assert_eq!(controls.model_current.as_deref(), Some("glm-5.3"));
    assert_eq!(controls.models.len(), 2);
    assert!(controls
        .models
        .iter()
        .all(|model| { model.reference["providerId"] == "bigmodel" }));
}

#[test]
fn turn_accumulates_text_deltas_and_ignores_unknown() {
    use zcode_tui::{AppServerEvent, AppServerTurn, TurnDelta};
    let mut turn = AppServerTurn::default();
    let ev = |kind: &str, delta: &str, done: bool| AppServerEvent {
        kind: kind.to_string(),
        delta: delta.to_string(),
        done,
        ..Default::default()
    };
    assert_eq!(turn.apply(&ev("text_start", "", false)), TurnDelta::None);
    assert_eq!(turn.apply(&ev("text_delta", "1", false)), TurnDelta::Text);
    assert_eq!(turn.apply(&ev("text_delta", "\n2", false)), TurnDelta::Text);
    assert_eq!(
        turn.apply(&ev("reasoning_delta", "thinking", false)),
        TurnDelta::Reasoning
    );
    assert_eq!(
        turn.apply(&ev("some_future_kind", "x", false)),
        TurnDelta::None
    );
    assert_eq!(turn.apply(&ev("finish", "", true)), TurnDelta::Done);
    assert_eq!(turn.text, "1\n2");
    assert_eq!(turn.reasoning, "thinking");
    assert!(turn.done);
}

#[test]
fn turn_tracks_tool_calls_start_to_result() {
    use zcode_tui::{decode_app_message, AppServerMessage, AppServerTurn, TurnDelta};
    let mut turn = AppServerTurn::default();
    let apply = |turn: &mut AppServerTurn, raw: &str| {
        let Some(AppServerMessage::Event(ev)) = decode_app_message(raw) else {
            panic!("expected event from {raw}");
        };
        turn.apply(&ev)
    };
    // A Read tool: start -> input delta -> full call -> result.
    assert_eq!(
        apply(
            &mut turn,
            r#"{"method":"session/event","params":{"payload":{"kind":"tool_input_start","toolCallId":"call_1","toolName":"Read","delta":"","done":false}}}"#
        ),
        TurnDelta::ToolStarted(0)
    );
    apply(
        &mut turn,
        r#"{"method":"session/event","params":{"payload":{"kind":"tool_input_delta","toolCallId":"call_1","delta":"{\"file_path\":\"notes.txt\"}","done":false}}}"#,
    );
    // The full tool_call re-sights the same id -> no new chip.
    assert_eq!(
        apply(
            &mut turn,
            r#"{"method":"session/event","params":{"payload":{"kind":"tool_call","toolCallId":"call_1","toolName":"Read","input":{},"delta":"","done":false}}}"#
        ),
        TurnDelta::None
    );
    assert_eq!(
        apply(
            &mut turn,
            r#"{"method":"session/event","params":{"payload":{"kind":"result","toolCallId":"call_1","duration":41,"result":{"success":true,"content":"1\thello\n2\tworld"}}}}"#
        ),
        TurnDelta::ToolFinished(0)
    );
    assert_eq!(turn.tools.len(), 1);
    let tool = &turn.tools[0];
    assert_eq!(tool.name, "Read");
    assert_eq!(tool.input, r#"{"file_path":"notes.txt"}"#);
    assert_eq!(tool.output, "1\thello\n2\tworld");
    assert_eq!(tool.duration_ms, Some(41));
    assert!(tool.success && tool.finished);
}

#[test]
fn tool_input_summary_condenses_json_args() {
    use zcode_tui::tool_input_summary;
    // Path values are basenamed and whitespace collapses.
    assert_eq!(
        tool_input_summary(r#"{"file_path":"/tmp/zcode/notes.txt"}"#),
        "notes.txt"
    );
    // Non-object / non-JSON falls back to the trimmed raw text.
    assert_eq!(tool_input_summary("  ls -la  "), "ls -la");
    assert_eq!(tool_input_summary(""), "");
    // Long summaries are capped with an ellipsis.
    let long = format!(r#"{{"q":"{}"}}"#, "x".repeat(80));
    assert!(tool_input_summary(&long).chars().count() <= 48);
}

#[test]
fn app_server_default_on_with_explicit_opt_out() {
    use zcode_tui::app_server_enabled;
    // Graduated: ON by default (unset), and legacy opt-in values still work.
    assert!(app_server_enabled(|_: &str| None));
    assert!(app_server_enabled(
        |k| (k == "ZCODE_TUI_APP_SERVER").then(|| "1".to_string())
    ));
    assert!(app_server_enabled(|_| Some("on".to_string())));
    assert!(app_server_enabled(|_| Some("true".to_string())));
    // Explicit opt-out values (case-insensitive, whitespace-tolerant).
    assert!(!app_server_enabled(|_| Some("0".to_string())));
    assert!(!app_server_enabled(|_| Some("off".to_string())));
    assert!(!app_server_enabled(|_| Some("FALSE".to_string())));
    assert!(!app_server_enabled(|_| Some(" no ".to_string())));
    // Unknown junk keeps the default (on), same as unset.
    assert!(app_server_enabled(|_| Some("maybe".to_string())));
}

#[test]
fn skyline_stretches_to_fill_width_exactly() {
    use unicode_width::UnicodeWidthStr;
    use zcode_tui::skyline_lines;
    for width in [70usize, 80, 100, 137] {
        let rows = skyline_lines(width);
        assert_eq!(rows.len(), 9, "8 silhouette rows + 1 horizon");
        for row in &rows {
            assert_eq!(row.width(), width, "row must fill exactly `width` columns");
        }
        // ZhiPU rests on the continuous horizon (last row); the nest mesh shows.
        assert!(rows[8].contains("ZhiPU"));
        assert!(rows.iter().any(|r| r.contains('╳')));
    }
    // Too narrow to lay out without overflow -> nothing (wordmark shows alone).
    assert!(skyline_lines(20).is_empty());
    assert!(skyline_lines(69).is_empty());
}

#[test]
fn braille_skyline_is_fixed_logo_width_with_brand() {
    use unicode_width::UnicodeWidthStr;
    use zcode_tui::{skyline_braille, SKYLINE_LOGO_W};
    let rows = skyline_braille();
    assert_eq!(rows.len(), 8, "7 silhouette rows + 1 horizon");
    for row in &rows {
        assert_eq!(
            row.width(),
            SKYLINE_LOGO_W,
            "every braille row is exactly the logo width so it centres under the wordmark"
        );
    }
    // Brand mark rests on the horizon; silhouette uses braille dots (U+28xx).
    assert!(rows[7].contains("ZhiPU"));
    assert!(rows
        .iter()
        .any(|r| r.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c))));
}

#[test]
fn skyline_mode_forces_and_autodetects() {
    use zcode_tui::{skyline_mode, SkylineMode};
    let force =
        |val: &'static str| move |key: &str| (key == "ZCODE_TUI_SKYLINE").then(|| val.to_string());
    assert_eq!(skyline_mode(force("wire")), SkylineMode::Wire);
    assert_eq!(skyline_mode(force("braille")), SkylineMode::Braille);
    assert_eq!(skyline_mode(force("off")), SkylineMode::None);
    // auto: UTF-8 locale -> braille (default), non-UTF-8 -> wireframe.
    let auto_utf8 = |key: &str| (key == "LANG").then(|| "en_US.UTF-8".to_string());
    let auto_c = |key: &str| (key == "LANG").then(|| "C".to_string());
    assert_eq!(skyline_mode(auto_utf8), SkylineMode::Braille);
    assert_eq!(skyline_mode(auto_c), SkylineMode::Wire);
}

#[test]
fn skyline_graphics_wanted_defaults_on_and_yields_to_text_modes() {
    use zcode_tui::skyline_graphics_wanted;
    let force =
        |val: &'static str| move |key: &str| (key == "ZCODE_TUI_SKYLINE").then(|| val.to_string());
    // Unset / auto / explicit `image` -> attempt the graphics protocol.
    assert!(skyline_graphics_wanted(|_: &str| None));
    assert!(skyline_graphics_wanted(force("auto")));
    assert!(skyline_graphics_wanted(force("image")));
    // Forcing a text/off mode opts out of the probe entirely.
    assert!(!skyline_graphics_wanted(force("wire")));
    assert!(!skyline_graphics_wanted(force("braille")));
    assert!(!skyline_graphics_wanted(force("off")));
    assert!(!skyline_graphics_wanted(force("none")));
    assert!(!skyline_graphics_wanted(force("0")));
    // Whitespace around a forced mode is tolerated.
    assert!(!skyline_graphics_wanted(force("  wire  ")));
}

#[test]
fn state_update_marks_turn_end_on_prompt_completed() {
    use zcode_tui::app_state_is_turn_end;
    let started = serde_json::json!({"reason":"prompt_started","patch":{"status":"running"}});
    let completed =
        serde_json::json!({"reason":"prompt_completed","patch":{"mode":{"current":"build"}}});
    let status_completed = serde_json::json!({"reason":"whatever","patch":{"status":"completed"}});
    // `idle`/`ready` are a settling state the kernel can emit *before* tokens
    // flow on a reused session — they must NOT finalize the turn (would show
    // "(no output)" prematurely). Only prompt_completed / status:completed do.
    let idle = serde_json::json!({"reason":"whatever","patch":{"status":"idle"}});
    let ready = serde_json::json!({"patch":{"status":"ready"}});
    assert!(!app_state_is_turn_end(&started));
    assert!(app_state_is_turn_end(&completed));
    assert!(app_state_is_turn_end(&status_completed));
    assert!(!app_state_is_turn_end(&idle));
    assert!(!app_state_is_turn_end(&ready));
}

#[test]
fn state_update_flags_abnormal_turn_end() {
    use zcode_tui::{app_state_is_turn_end, app_state_turn_error};
    // Abnormal terminal states end the turn (via a distinct path) rather than
    // hanging on the 600s backstop — reported via reason or patch/status.
    let errored = serde_json::json!({"reason":"error","patch":{"status":"running"}});
    let aborted = serde_json::json!({"patch":{"status":"Aborted"}}); // case-insensitive
    let failed = serde_json::json!({"reason":"failed"});
    let running = serde_json::json!({"reason":"prompt_started","patch":{"status":"running"}});
    assert_eq!(app_state_turn_error(&errored).as_deref(), Some("error"));
    assert_eq!(app_state_turn_error(&aborted).as_deref(), Some("Aborted"));
    assert_eq!(app_state_turn_error(&failed).as_deref(), Some("failed"));
    assert!(app_state_turn_error(&running).is_none());
    // An abnormal end is not a normal completion.
    assert!(!app_state_is_turn_end(&errored));
}

#[test]
fn state_watermark_found_anywhere_in_tree() {
    use zcode_tui::{app_state_context_values, app_state_total_tokens, app_state_watermark};
    // Nested under an arbitrary path — the walk should still find the pair.
    let params = serde_json::json!({
        "patch": { "context": { "contextUsed": 1234, "contextWindow": 200000 } },
        "reason": "turn",
    });
    assert_eq!(app_state_watermark(&params), Some((1234, 200000)));

    // Incremental patches commonly update only the used side. The stable
    // model window must not be required for this live update to survive.
    let partial = serde_json::json!({
        "patch": {"projection": {"contextUsed": 4321}}
    });
    assert_eq!(app_state_context_values(&partial), (Some(4321), None));

    let live_projection = serde_json::json!({
        "snapshot": {"projection": {"contextUsed": 4321, "totalTokenCount": 9876}}
    });
    assert_eq!(app_state_total_tokens(&live_projection), Some(9876));
    assert_eq!(
        app_state_total_tokens(&serde_json::json!({"usage": {"total": 999999}})),
        None
    );

    let window_only = serde_json::json!({
        "snapshot": {"projection": {"contextWindow": 200000}}
    });
    assert_eq!(app_state_context_values(&window_only), (None, Some(200000)));
    // Alternate key names are accepted too.
    let alt = serde_json::json!({ "usage": { "used": 10, "total": 100 } });
    assert_eq!(app_state_watermark(&alt), Some((10, 100)));
    // No pair / zero window -> None (caller keeps the last value).
    assert_eq!(
        app_state_watermark(&serde_json::json!({ "status": "running" })),
        None
    );
    assert_eq!(
        app_state_watermark(&serde_json::json!({ "used": 5, "window": 0 })),
        None
    );
}

#[test]
fn unavailable_reasons_display() {
    use zcode_tui::AppServerUnavailable;
    assert!(AppServerUnavailable::Spawn("no bin".into())
        .to_string()
        .contains("did not start"));
    assert!(AppServerUnavailable::Disconnected
        .to_string()
        .contains("closed"));
}

// ---- streaming-attachments-and-comfort batch -----------------------------

#[test]
fn send_attachments_map_mentions_by_extension() {
    use zcode_tui::{app_send_params_with_attachments, build_send_attachments};
    let dir = std::env::temp_dir().join(format!("zcode-attach-{}", std::process::id()));
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("notes.txt"), "hello sentinel").unwrap();
    fs::write(dir.join("shot.PNG"), [0u8; 9]).unwrap();
    fs::write(dir.join("sub/config.weird"), "x").unwrap();

    let mentions = vec![
        "notes.txt".to_string(),
        "shot.PNG".to_string(),
        "sub/config.weird".to_string(),
        "missing.txt".to_string(), // metadata unreadable -> skipped
    ];
    let attachments = build_send_attachments(&mentions, &dir);
    assert_eq!(attachments.len(), 3);

    let file = &attachments[0];
    assert_eq!(file["kind"], "file");
    assert_eq!(file["filename"], "notes.txt");
    assert_eq!(file["mimeType"], "text/plain");
    // kind:"file" REQUIRES sizeBytes (kernel Pwt schema is strict).
    assert_eq!(file["sizeBytes"], 14);
    assert!(std::path::Path::new(file["localPath"].as_str().unwrap()).ends_with("notes.txt"));
    assert!(file.get("dataBase64").is_none());

    let image = &attachments[1];
    assert_eq!(image["kind"], "image");
    assert_eq!(image["mimeType"], "image/png"); // extension case-folded

    let unknown = &attachments[2];
    assert_eq!(unknown["kind"], "file");
    assert_eq!(unknown["mimeType"], "text/plain"); // fallback
    assert_eq!(unknown["filename"], "config.weird"); // basename, not the path

    // Empty attachments -> byte-identical to the plain send params.
    let plain = app_send_params_with_attachments("sess_1", "hi", &[]);
    assert_eq!(
        plain,
        serde_json::json!({"sessionId":"sess_1","content":"hi"})
    );
    let with = app_send_params_with_attachments("sess_1", "hi", &attachments);
    assert_eq!(with["attachments"].as_array().unwrap().len(), 3);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn mcp_servers_param_builds_kernel_shapes() {
    use zcode_tui::{mcp_servers_param, with_mcp_servers, McpConfig};
    let project: McpConfig = serde_json::from_str(
        r#"{"mcpServers":{
            "files":{"command":"npx","args":["-y","server-files"],"env":{"TOKEN":"t1"}},
            "off":{"command":"nope","disabled":true},
            "shared":{"command":"project-wins"}
        }}"#,
    )
    .unwrap();
    let user: McpConfig = serde_json::from_str(
        r#"{"mcpServers":{
            "web":{"type":"sse","url":"https://mcp.example/sse","headers":{"X-Key":"k"}},
            "shared":{"command":"user-loses"}
        }}"#,
    )
    .unwrap();

    let servers = mcp_servers_param(&project, &user).unwrap();
    let list = servers.as_array().unwrap();
    assert_eq!(list.len(), 3); // files, shared, web — disabled skipped

    let files = list.iter().find(|s| s["name"] == "files").unwrap();
    // stdio shape has NO type key (kernel $xe union is strict).
    assert!(files.get("type").is_none());
    assert_eq!(files["command"], "npx");
    assert_eq!(files["args"], serde_json::json!(["-y", "server-files"]));
    assert_eq!(
        files["env"],
        serde_json::json!([{"name":"TOKEN","value":"t1"}])
    );

    let shared = list.iter().find(|s| s["name"] == "shared").unwrap();
    assert_eq!(shared["command"], "project-wins");

    let web = list.iter().find(|s| s["name"] == "web").unwrap();
    assert_eq!(web["type"], "sse");
    assert_eq!(web["url"], "https://mcp.example/sse");
    assert_eq!(
        web["headers"],
        serde_json::json!([{"name":"X-Key","value":"k"}])
    );

    // Empty configs -> None; with_mcp_servers(None) leaves params untouched.
    assert!(mcp_servers_param(&McpConfig::default(), &McpConfig::default()).is_none());
    let base = serde_json::json!({"sessionId":"sess_1"});
    assert_eq!(with_mcp_servers(base.clone(), None), base);
    let with = with_mcp_servers(base, Some(servers));
    assert_eq!(with["mcpServers"].as_array().unwrap().len(), 3);
}

#[test]
fn zcode_336_tool_policy_attaches_to_create_and_resume_only_when_nonempty() {
    use zcode_tui::{app_create_params, app_resume_params, with_tool_policy};

    let allow = vec!["Read".to_string(), "Glob".to_string()];
    let deny = vec!["Bash(git *)".to_string()];
    let create = with_tool_policy(app_create_params("/proj"), &allow, &deny);
    assert_eq!(create["toolAllowlist"], serde_json::json!(["Read", "Glob"]));
    assert_eq!(create["toolDenylist"], serde_json::json!(["Bash(git *)"]));

    let resume = with_tool_policy(app_resume_params("sess_1", None), &allow, &deny);
    assert_eq!(resume["sessionId"], "sess_1");
    assert_eq!(resume["toolAllowlist"], serde_json::json!(["Read", "Glob"]));

    let bare = app_create_params("/proj");
    assert_eq!(with_tool_policy(bare.clone(), &[], &[]), bare);
}

#[test]
fn resume_messages_replay_filters_and_truncates() {
    use zcode_tui::parse_resume_messages;
    // Shape as captured live 2026-07-07: info.role + parts[].type/text.
    let long = "长".repeat(450);
    let result = serde_json::json!({"messages": [
        {"info": {"role": "user"}, "parts": [
            {"type": "text", "text": "What is the secret phrase?"},
            {"type": "file", "filename": "attach_me.txt"}
        ]},
        {"info": {"role": "assistant"}, "parts": [
            {"type": "text", "text": long},
            {"type": "step-start"}, {"type": "step-finish"}
        ]},
        {"info": {"role": "assistant"}, "parts": [
            {"type": "reasoning", "text": "thinking only"}
        ]},
    ]});
    let replay = parse_resume_messages(&result, 6, 400);
    assert_eq!(replay.len(), 2); // reasoning-only message skipped
    assert_eq!(replay[0].role, "user");
    assert_eq!(replay[0].preview, "What is the secret phrase?");
    assert_eq!(replay[1].role, "assistant");
    assert_eq!(replay[1].preview.chars().count(), 401); // 400 + '…'
    assert!(replay[1].preview.ends_with('…'));

    // Limit keeps the LAST messages.
    let many = serde_json::json!({"messages": (0..9).map(|i| serde_json::json!(
        {"info": {"role": "user"}, "parts": [{"type": "text", "text": format!("m{i}")}]}
    )).collect::<Vec<_>>()});
    let tail = parse_resume_messages(&many, 6, 400);
    assert_eq!(tail.len(), 6);
    assert_eq!(tail[0].preview, "m3");
    assert_eq!(tail[5].preview, "m8");

    // Missing messages -> empty.
    assert!(parse_resume_messages(&serde_json::json!({}), 6, 400).is_empty());
}

#[test]
fn osc52_sequence_encodes_and_caps() {
    use zcode_tui::{base64_encode, osc52_copy_sequence};
    // RFC 4648 vectors.
    assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64_encode(b""), "");

    let seq = osc52_copy_sequence("hello", 100_000).unwrap();
    assert_eq!(seq, "\x1b]52;c;aGVsbG8=\x07");

    // Cap truncates the SOURCE on a char boundary; sequence stays valid.
    let capped = osc52_copy_sequence(&"号".repeat(100), 40).unwrap();
    let b64 = capped
        .strip_prefix("\x1b]52;c;")
        .unwrap()
        .strip_suffix('\x07')
        .unwrap();
    assert!(b64.len() <= 40);
    assert!(b64.len().is_multiple_of(4)); // valid base64 framing

    assert!(osc52_copy_sequence("", 100_000).is_none());
}

#[test]
fn session_event_type_passthrough_and_checkpoint_count() {
    use zcode_tui::{decode_app_message, AppServerMessage, AppServerTurn, TurnDelta};
    // checkpoint.created has NO payload.kind — params.type passes through
    // (previously dropped as unparseable).
    let line = r#"{"method":"session/event","params":{"type":"checkpoint.created",
        "payload":{"checkpointId":"chk_1","scope":"workspace","fileCount":2}}}"#
        .replace('\n', " ");
    let Some(AppServerMessage::Event(event)) = decode_app_message(&line) else {
        panic!("checkpoint event not decoded");
    };
    assert_eq!(event.kind, "checkpoint.created");
    assert_eq!(event.file_count, Some(2));

    // Streaming payloads keep their own kind even when params.type is set.
    let stream = r#"{"method":"session/event","params":{"type":"model.streaming",
        "payload":{"kind":"text_delta","delta":"hi"}}}"#
        .replace('\n', " ");
    let Some(AppServerMessage::Event(event)) = decode_app_message(&stream) else {
        panic!("stream event not decoded");
    };
    assert_eq!(event.kind, "text_delta");

    // Neither kind nor type -> still skipped quietly.
    let bare = r#"{"method":"session/event","params":{"payload":{"x":1}}}"#;
    assert!(decode_app_message(bare).is_none());

    // Turn accumulator: checkpoints and fileCount sum; no UI delta.
    let mut turn = AppServerTurn::default();
    let mut chk = zcode_tui::AppServerEvent {
        kind: "checkpoint.created".to_string(),
        file_count: Some(2),
        ..Default::default()
    };
    assert_eq!(turn.apply(&chk), TurnDelta::None);
    chk.file_count = None; // tolerated: counts the checkpoint, adds 0 files
    assert_eq!(turn.apply(&chk), TurnDelta::None);
    assert_eq!(turn.checkpoints, 2);
    assert_eq!(turn.files_changed, 2);
}

#[test]
fn close_params_and_notify_config() {
    use zcode_tui::{app_close_params, parse_ui_config as parse};
    // session/close params pinned live 2026-07-07: {sessionId} strict.
    assert_eq!(
        app_close_params("sess_9"),
        serde_json::json!({"sessionId":"sess_9"})
    );

    assert_eq!(parse("notify = off").notify, Some(false));
    assert_eq!(parse("notify = on").notify, Some(true));
    assert_eq!(parse("notify = maybe").notify, None); // bad value ignored
    assert_eq!(parse("").notify, None); // default: bell enabled
}

#[test]
fn debug_log_lines_redact_params() {
    use zcode_tui::{log_line_inbound, log_line_outbound, AppServerMessage};
    // Outbound: METHOD NAME ONLY — a resume carries runtimeModel/apiKey in
    // its params, which must never appear.
    let line = log_line_outbound("session/resume", 7);
    assert_eq!(line, "-> session/resume (id 7)");
    assert!(!line.contains("apiKey") && !line.contains("runtimeModel"));

    // Inbound summaries: structural only, errors truncated.
    let ok = AppServerMessage::Response {
        id: 3,
        result: Some(serde_json::json!({"secret": "value-not-logged"})),
        error: None,
    };
    let logged = log_line_inbound(&ok);
    assert_eq!(logged, "<- response id 3 ok");
    assert!(!logged.contains("value-not-logged"));

    let err = AppServerMessage::Response {
        id: 4,
        result: None,
        error: Some("x".repeat(500)),
    };
    assert!(log_line_inbound(&err).chars().count() < 200);

    let event = AppServerMessage::Event(zcode_tui::AppServerEvent {
        kind: "text_delta".to_string(),
        delta: "top secret token".to_string(),
        ..Default::default()
    });
    let logged = log_line_inbound(&event);
    assert_eq!(logged, "<- event text_delta +16b"); // length, not content

    let state = AppServerMessage::StateUpdated(
        serde_json::json!({"reason":"prompt_completed","patch":{"big":"blob"}}),
    );
    assert_eq!(
        log_line_inbound(&state),
        "<- state.updated reason=prompt_completed"
    );
}

#[test]
fn copy_leader_key_and_local_command_routed() {
    // Ctrl+X y -> CopyLastReply; /copy classifies as a local command.
    assert_eq!(
        leader_action_for_key('y'),
        Some(LeaderAction::CopyLastReply)
    );
    let action = classify_input("/copy").unwrap();
    assert_eq!(action, InputAction::Local(vec!["copy".to_string()]));
}

// ---- session-rewind (payload shapes pinned live 2026-07-07, kernel 0.15.0) ----

#[test]
fn checkpoint_event_surfaces_id_for_rewind_targets() {
    use zcode_tui::{checkpoint_short_id, decode_app_message, AppServerMessage};
    // Exact spike capture (spike_a.py): checkpoint.created payload keys.
    let line = r#"{"method":"session/event","params":{"deliveryKind":"desktop-continuous",
        "eventId":"bc388f32","payload":{"checkpointId":"checkpoint_90c0d5df-3e2f-4c13-b826-4842d2bd1da7",
        "messageId":"msg_a","targetMessageId":"msg_a","toolMessageId":"msg_b","scope":"workspace",
        "snapshotRef":"zcode-artifact://sess_x/tool-result-1","diffRef":"zcode-artifact://sess_x/tool-result-1",
        "fileCount":1},"seq":18,"sessionId":"sess_x","type":"checkpoint.created"}}"#
        .replace('\n', " ");
    let Some(AppServerMessage::Event(event)) = decode_app_message(&line) else {
        panic!("checkpoint event not decoded");
    };
    assert_eq!(event.kind, "checkpoint.created");
    assert_eq!(
        event.checkpoint_id.as_deref(),
        Some("checkpoint_90c0d5df-3e2f-4c13-b826-4842d2bd1da7")
    );
    assert_eq!(event.file_count, Some(1));
    // targetMessageId feeds the conversation-scope leg's message target.
    assert_eq!(event.target_message_id.as_deref(), Some("msg_a"));
    assert_eq!(
        checkpoint_short_id("checkpoint_90c0d5df-3e2f-4c13-b826-4842d2bd1da7"),
        "90c0d5df"
    );
    // Missing checkpointId: still a countable event, just not a target.
    let bare = r#"{"method":"session/event","params":{"type":"checkpoint.created",
        "payload":{"fileCount":1}}}"#
        .replace('\n', " ");
    let Some(AppServerMessage::Event(event)) = decode_app_message(&bare) else {
        panic!("bare checkpoint event not decoded");
    };
    assert_eq!(event.checkpoint_id, None);
}

#[test]
fn rewind_triggered_event_surfaces_strategy_and_reason() {
    use zcode_tui::{decode_app_message, AppServerMessage};
    // Spike: the failed-rewind event (success envelope lies; this doesn't).
    let line = r#"{"method":"session/event","params":{"payload":{"rewindId":"rewind_f03466de",
        "scope":"workspace","strategy":"unavailable","targetCheckpointId":"checkpoint_does-not-exist",
        "reason":"target_checkpoint_not_found"},"type":"rewind.triggered"}}"#
        .replace('\n', " ");
    let Some(AppServerMessage::Event(event)) = decode_app_message(&line) else {
        panic!("rewind.triggered not decoded");
    };
    assert_eq!(event.kind, "rewind.triggered");
    assert_eq!(event.strategy.as_deref(), Some("unavailable"));
    assert_eq!(event.reason.as_deref(), Some("target_checkpoint_not_found"));
}

#[test]
fn rewind_params_match_pinned_shapes() {
    use zcode_tui::{app_file_rewind_params, app_rewind_params, RewindTarget};
    // previewFileRewind/applyFileRewind: {sessionId, target} (ZodError names
    // exactly those on empty params).
    assert_eq!(
        app_file_rewind_params("sess_1", &RewindTarget::LatestCheckpoint),
        serde_json::json!({"sessionId":"sess_1","target":{"kind":"latestCheckpoint"}})
    );
    assert_eq!(
        app_file_rewind_params("sess_1", &RewindTarget::Checkpoint("checkpoint_a".into())),
        serde_json::json!({"sessionId":"sess_1",
            "target":{"kind":"checkpoint","checkpointId":"checkpoint_a"}})
    );
    // session/rewind: {sessionId, target, scope}; the TUI only ever sends
    // scope:"conversation" (file scopes go through applyFileRewind — pinned
    // live: session/rewind force-applies over external modifications).
    assert_eq!(
        app_rewind_params("sess_1", &RewindTarget::Turn(0), "conversation"),
        serde_json::json!({"sessionId":"sess_1",
            "target":{"kind":"turn","turnIndex":0},"scope":"conversation"})
    );
    assert_eq!(RewindTarget::LatestCheckpoint.label(), "latest checkpoint");
    assert_eq!(
        RewindTarget::Checkpoint("checkpoint_90c0d5df-xyz".into()).label(),
        "checkpoint 90c0d5df"
    );
}

#[test]
fn v4_353_subscribe_command_and_rewind_shapes() {
    use zcode_tui::{
        v4_command_params, v4_conversation_subscribe_params, v4_file_rewind_preview_params,
        v4_rewind_target, V4CommandBase,
    };
    assert_eq!(
        v4_conversation_subscribe_params("sess_1", "zcode-tui-7"),
        serde_json::json!({
            "topic":"conversation/sess_1",
            "connectionId":"zcode-tui-7",
            "clientMode":"desktop-continuous",
            "visibility":"foreground"
        })
    );
    assert_eq!(
        v4_command_params(
            "cmd_1",
            "zcode-tui-7",
            "sess_1",
            "setFollowupMode",
            serde_json::json!({"mode":"guide"}),
            V4CommandBase::Revision(12),
            99,
        ),
        serde_json::json!({
            "commandId":"cmd_1","clientId":"zcode-tui-7","sessionId":"sess_1",
            "baseRevision":12,"type":"setFollowupMode","payload":{"mode":"guide"},
            "issuedAt":99
        })
    );
    assert_eq!(
        v4_file_rewind_preview_params("sess_1", 6, "msg_x", 26, "epoch_x"),
        serde_json::json!({
            "sessionId":"sess_1","target":{"rowId":6,"entityId":"msg_x"},
            "baseRevision":26,"baseLogEpoch":"epoch_x"
        })
    );
    assert_eq!(
        v4_rewind_target(6, "msg_x"),
        serde_json::json!({"rowId":6,"entityId":"msg_x"})
    );
}

#[test]
fn v4_frames_track_revision_rows_and_semantic_guide_delivery() {
    use zcode_tui::V4ConversationState;
    let mut state = V4ConversationState::default();
    let snapshot = serde_json::json!({
        "wireVersion":3,"kind":"complete","frame":{"payload":{"kind":"snapshot","snapshot":{
            "revision":0,"logEpoch":"epoch_a","inputRouting":{"mode":"startNow"},
            "config":{"followupMode":"queue"},
            "availability":{"setFollowupMode":{"allowed":true}},
            "queue":{"items":[]},"rows":{"window":[]}
        }}}
    });
    assert!(state.apply_frame(&snapshot).deliveries.is_empty());
    assert_eq!(state.revision, Some(0));
    assert_eq!(state.log_epoch.as_deref(), Some("epoch_a"));
    assert_eq!(state.input_routing.as_deref(), Some("startNow"));

    let delta = serde_json::json!({
        "wireVersion":3,"kind":"complete","frame":{"payload":{"kind":"deltas","deltas":[
            {"op":"row.appended","row":{"rowId":6,"entityId":"msg_x","kind":"turnHeader",
                "state":"completedSuccess","fileChanges":{"files":1,"additions":1,"deletions":1,
                "state":"active"},"actions":{"canRewindFiles":true}}},
            {"op":"state.updated","patch":{"config":{"followupMode":"guide"},
                "inputRouting":{"mode":"guide"},"revision":27,
                "queue":{"items":[{"sourceCommandId":"cmd_text",
                    "delivery":{"requested":"guide","admitted":"guide"}}]}}}
        ]}}
    });
    let effect = state.apply_frame(&delta);
    assert_eq!(state.revision, Some(27));
    assert_eq!(state.followup_mode.as_deref(), Some("guide"));
    assert_eq!(state.delivery_for("cmd_text"), Some("guide"));
    assert_eq!(effect.deliveries, vec![("cmd_text".into(), "guide".into())]);
    let rows = state.rewind_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].row_id, 6);
    assert_eq!(rows[0].entity_id, "msg_x");

    let reverted = serde_json::json!({
        "frame":{"payload":{"kind":"deltas","deltas":[
            {"op":"row.upserted","row":{"rowId":6,"entityId":"msg_x","kind":"turnHeader",
                "state":"completedSuccess","fileChanges":{"files":1,"additions":1,"deletions":1,
                "state":"reverted"},"actions":{"canRewindFiles":true}}}
        ]}}
    });
    state.apply_frame(&reverted);
    assert!(state.rewind_rows().is_empty());
}

#[test]
fn v4_command_ack_requires_semantic_status() {
    use zcode_tui::parse_v4_command_ack;
    let accepted = parse_v4_command_ack(&serde_json::json!({
        "commandId":"cmd_1","status":"accepted","revisionAtDecision":3,
        "result":{"type":"inputDisposition","delivery":"guide"}
    }))
    .unwrap();
    assert!(accepted.accepted());
    assert_eq!(accepted.input_delivery(), Some("guide"));

    let stale = parse_v4_command_ack(&serde_json::json!({
        "commandId":"cmd_2","status":"stale","reasonCode":"proto.staleRevision",
        "revisionAtDecision":2
    }))
    .unwrap();
    assert!(!stale.accepted());
    assert_eq!(stale.reason_code.as_deref(), Some("proto.staleRevision"));
}

#[test]
fn v4_frame_decodes_and_protocol_log_stays_structural() {
    use zcode_tui::{
        decode_app_message, log_line_inbound, log_line_outbound_request, AppServerMessage,
    };
    let raw = r#"{"method":"v4/conversation/frame","params":{"kind":"complete","frame":{"payload":{"kind":"snapshot","snapshot":{"revision":7}}}}}"#;
    let message = decode_app_message(raw).expect("v4 frame decodes");
    assert!(matches!(message, AppServerMessage::V4Frame(_)));
    assert_eq!(
        log_line_inbound(&message),
        "<- v4/conversation/frame kind=snapshot rev=7"
    );
    let outbound = log_line_outbound_request(
        "v4/command",
        9,
        &serde_json::json!({
            "type":"sendText","baseRevision":7,
            "payload":{"text":"TOP SECRET"}
        }),
    );
    assert_eq!(outbound, "-> v4/command type=sendText rev=7 (id 9)");
    assert!(!outbound.contains("TOP SECRET"));
}

#[test]
fn rewind_preview_parses_safe_and_unsafe_shapes() {
    use zcode_tui::parse_rewind_preview;
    // Exact spike result: clean preview.
    let safe = serde_json::json!({
        "canApply": true, "ignoredFiles": [],
        "safeFiles": [{"action":"restore","operationCount":1,"path":"/ws/a.txt","toolNames":["Write"]}],
        "unsafeFiles": [], "sessionId": "sess_x", "target": {"kind":"latestCheckpoint"}
    });
    let preview = parse_rewind_preview(&safe).expect("safe preview");
    assert!(preview.can_apply);
    assert_eq!(preview.safe.len(), 1);
    assert_eq!(preview.safe[0].path, "/ws/a.txt");
    assert_eq!(preview.safe[0].note, "restore");
    assert_eq!(preview.safe[0].tools, "Write");
    assert!(preview.unsafe_files.is_empty());

    // Exact spike result after external tamper (currentHash can also be the
    // string "missing" when the file was deleted).
    let unsafe_result = serde_json::json!({
        "canApply": false, "ignoredFiles": [], "safeFiles": [],
        "unsafeFiles": [{"operationCount":1,"path":"/ws/a.txt","reason":"external_modified",
            "toolNames":["Write"],"expectedHash":"27dd8e","currentHash":"missing"}],
        "sessionId": "sess_x", "target": {"kind":"checkpoint","checkpointId":"checkpoint_a"}
    });
    let preview = parse_rewind_preview(&unsafe_result).expect("unsafe preview");
    assert!(!preview.can_apply);
    assert_eq!(preview.unsafe_files.len(), 1);
    assert_eq!(preview.unsafe_files[0].note, "external_modified");

    // Not a preview shape (e.g. an unrelated result) -> None.
    assert!(parse_rewind_preview(&serde_json::json!({"response":"ok"})).is_none());
}

#[test]
fn apply_file_rewind_refusal_and_success_parse() {
    use zcode_tui::parse_apply_file_rewind;
    // Exact spike result: refusal keeps applied:false + embedded preview.
    let refused = serde_json::json!({
        "applied": false,
        "preview": {"canApply": false, "ignoredFiles": [], "safeFiles": [],
            "unsafeFiles": [{"operationCount":1,"path":"/ws/a.txt","reason":"external_modified",
                "toolNames":["Write"],"expectedHash":"27dd8e","currentHash":"23bd61"}],
            "sessionId":"sess_x","target":{"kind":"latestCheckpoint"}},
        "response": "File rewind was not applied because at least one file is unsafe."
    });
    let outcome = parse_apply_file_rewind(&refused);
    assert!(!outcome.applied);
    assert!(outcome.response.contains("not applied"));
    assert_eq!(outcome.unsafe_files.len(), 1);
    assert_eq!(outcome.unsafe_files[0].path, "/ws/a.txt");

    let applied = serde_json::json!({
        "applied": true,
        "preview": {"canApply": true, "ignoredFiles": [], "safeFiles": [], "unsafeFiles": []},
        "response": "Restored 1 file."
    });
    assert!(parse_apply_file_rewind(&applied).applied);
    // Missing `applied` (unknown shape) must NOT read as success.
    assert!(!parse_apply_file_rewind(&serde_json::json!({"response":"?"})).applied);
}

#[test]
fn rewind_outcome_judged_by_trigger_event_not_envelope() {
    use zcode_tui::rewind_failure;
    // Real rewind: strategy active_chain -> success.
    assert_eq!(
        rewind_failure(
            Some("active_chain"),
            Some("target_in_active_chain"),
            "Rewound…"
        ),
        None
    );
    // Pinned live: nonexistent checkpoint returns a SUCCESS envelope whose
    // response reads "…was not found." — only the event tells the truth.
    let failure = rewind_failure(
        Some("unavailable"),
        Some("target_checkpoint_not_found"),
        "Checkpoint checkpoint_does-not-exist was not found.",
    )
    .expect("unavailable is a failure");
    assert!(failure.contains("was not found"));
    // unavailable with no response text falls back to the reason.
    let bare = rewind_failure(Some("unavailable"), Some("target_checkpoint_not_found"), "")
        .expect("failure");
    assert!(bare.contains("target_checkpoint_not_found"));
    // No rewind.triggered observed at all: never claim success.
    assert!(rewind_failure(None, None, "whatever").is_some());
}

#[test]
fn rewind_is_a_local_command() {
    let action = classify_input("/rewind").unwrap();
    assert_eq!(action, InputAction::Local(vec!["rewind".to_string()]));
}

#[test]
fn conversation_scope_targets_translate_to_message_kind() {
    use zcode_tui::{conversation_target, CheckpointEntry, RewindTarget};
    // Pinned live 2026-07-09: session/rewind COERCES checkpoint-kind targets
    // to a forced workspace (file) rewind even under scope:"conversation"
    // (rewind.triggered came back scope:"workspace" and the file was
    // deleted). Only {kind:"message"} targets honor the conversation scope,
    // so conversation legs must translate via the checkpoint's
    // targetMessageId.
    let checkpoints = vec![
        CheckpointEntry {
            id: "checkpoint_a".into(),
            files: 1,
            message_id: Some("msg_a".into()),
        },
        CheckpointEntry {
            id: "checkpoint_b".into(),
            files: 2,
            message_id: Some("msg_b".into()),
        },
    ];
    assert_eq!(
        conversation_target(
            &RewindTarget::Checkpoint("checkpoint_a".into()),
            &checkpoints
        ),
        Some(RewindTarget::Message("msg_a".into()))
    );
    // latestCheckpoint -> the newest captured entry's message.
    assert_eq!(
        conversation_target(&RewindTarget::LatestCheckpoint, &checkpoints),
        Some(RewindTarget::Message("msg_b".into()))
    );
    assert_eq!(
        RewindTarget::Message("msg_a".into()).to_json(),
        serde_json::json!({"kind":"message","messageId":"msg_a"})
    );
    // Unknown checkpoint or missing message id -> None (the caller refuses
    // the conversation leg rather than sending a coercible target).
    assert_eq!(
        conversation_target(
            &RewindTarget::Checkpoint("checkpoint_x".into()),
            &checkpoints
        ),
        None
    );
    let no_msg = vec![CheckpointEntry {
        id: "checkpoint_a".into(),
        files: 1,
        message_id: None,
    }];
    assert_eq!(
        conversation_target(&RewindTarget::LatestCheckpoint, &no_msg),
        None
    );
}

#[test]
fn decodes_background_task_event() {
    use zcode_tui::{decode_app_message, AppServerMessage};
    // ZCode 3.3.4 background tasks: the event must decode so a future
    // app-server delivery is never silently dropped.
    let line = r#"{"method":"session/event","params":{"type":"background_task_started","payload":{"taskId":"bg-1","toolName":"Bash","toolCallId":"t9","command":"sleep 12","status":"running","pid":4242}}}"#;
    match decode_app_message(line).expect("decodes") {
        AppServerMessage::Event(event) => {
            assert_eq!(event.kind, "background_task_started");
            assert_eq!(event.command.as_deref(), Some("sleep 12"));
            assert_eq!(event.status.as_deref(), Some("running"));
            assert_eq!(event.pid, Some(4242));
            assert_eq!(event.task_id.as_deref(), Some("bg-1"));
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn subagents_snapshot_keeps_protocol_identifiers_and_work_kinds_separate() {
    let rows = zcode_tui::parse_subagents_result(&serde_json::json!({
        "session": {
            "subagents": [{
                "taskId": "task-agent", "childSessionId": "child-7",
                "agentId": "agent-7", "toolCallId": "tool-7",
                "title": "researcher", "status": "running", "revision": 9
            }],
            "backgroundWorks": [{
                "taskId": "task-bash", "toolCallId": "tool-bash",
                "command": "cargo test", "status": "completed"
            }]
        }
    }));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].kind, "subagent");
    assert_eq!(rows[0].task_id.as_deref(), Some("task-agent"));
    assert_eq!(rows[0].child_session_id.as_deref(), Some("child-7"));
    assert_eq!(rows[0].agent_id.as_deref(), Some("agent-7"));
    assert_eq!(rows[0].tool_call_id.as_deref(), Some("tool-7"));
    assert_eq!(rows[1].kind, "background");
    assert_eq!(rows[1].command.as_deref(), Some("cargo test"));
}

#[test]
fn parses_pinned_zcode_0163_running_and_ended_subagent_shape() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/zcode-0.16.3-child-transcript.json")).unwrap();
    let running = zcode_tui::parse_subagents_result(&fixture["running_subagents_result"]);
    assert_eq!(running.len(), 1);
    assert_eq!(
        running[0].child_session_id.as_deref(),
        Some("child-running")
    );
    assert_eq!(running[0].agent_id.as_deref(), Some("agent-running"));
    assert_eq!(running[0].status.as_deref(), Some("running"));
    assert_eq!(running[0].revision, Some(1));

    let ended = zcode_tui::parse_subagents_result(&fixture["ended_subagents_result"]);
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].child_session_id.as_deref(), Some("child-ended"));
    assert_eq!(ended[0].status.as_deref(), Some("success"));
    assert_eq!(
        ended[0].summary.as_deref(),
        Some("summary available from session/subagents")
    );
}

#[test]
fn background_cancel_uses_strict_task_id_shape_and_parses_outcome() {
    assert_eq!(
        zcode_tui::app_cancel_background_task_params("parent-1", "task-exact"),
        serde_json::json!({"sessionId": "parent-1", "taskId": "task-exact"})
    );
    let outcome = zcode_tui::parse_cancel_background_task_result(&serde_json::json!({
        "cancelled": false,
        "reason": "background_task_not_found",
        "status": "lost",
        "taskId": "task-exact"
    }))
    .unwrap();
    assert_eq!(outcome.task_id, "task-exact");
    assert!(!outcome.cancelled);
    assert_eq!(outcome.reason.as_deref(), Some("background_task_not_found"));
}

#[test]
fn v4_snapshot_and_subagent_lifecycle_event_are_decoded() {
    let rows = zcode_tui::parse_v4_agent_snapshots(&serde_json::json!({
        "frame": {"payload": {
            "kind": "snapshot",
            "snapshot": {
                "subagents": [{"childSessionId": "child-v4", "state": "running"}],
                "backgroundWorks": [{"taskId": "bg-v4", "canCancel": true}]
            }
        }}
    }));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].child_session_id.as_deref(), Some("child-v4"));
    assert_eq!(rows[1].cancellable, Some(true));

    let line = serde_json::json!({
        "method": "session/event",
        "params": {
            "type": "subagent_spawned",
            "payload": {
                "childSessionId": "child-event", "agentId": "agent-event",
                "taskId": "task-event", "toolCallId": "tool-event",
                "summary": "inspect architecture", "revision": 12
            }
        }
    })
    .to_string();
    let Some(zcode_tui::AppServerMessage::Event(event)) = zcode_tui::decode_app_message(&line)
    else {
        panic!("expected decoded subagent lifecycle event");
    };
    assert_eq!(event.kind, "subagent_spawned");
    assert_eq!(event.child_session_id.as_deref(), Some("child-event"));
    assert_eq!(event.agent_id.as_deref(), Some("agent-event"));
    assert_eq!(event.summary.as_deref(), Some("inspect architecture"));
    assert_eq!(event.revision, Some(12));
}
