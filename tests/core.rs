use std::fs;

use zcode_tui::{
    build_prompt_command, classify_input, command_palette_rows, leader_action_for_key,
    load_mcp_config, parse_cli_args, save_mcp_config, slash_suggestions, strip_ansi, InputAction,
    LeaderAction, McpServer,
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
fn command_palette_exposes_common_commands() {
    let rows = command_palette_rows();

    assert!(rows.iter().any(|row| row.contains("/goal")));
    assert!(rows.iter().any(|row| row.contains("/skills list")));
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
        },
    );
    save_mcp_config(&config_path, &config).unwrap();

    let raw = fs::read_to_string(&config_path).unwrap();
    assert!(raw.contains("\"mcpServers\""));
    assert!(raw.contains("\"fs\""));
    assert!(raw.contains("@modelcontextprotocol/server-filesystem"));

    let reloaded = load_mcp_config(&config_path).unwrap();
    assert_eq!(reloaded.servers["fs"].command, "npx");
    assert_eq!(
        reloaded.servers["fs"].args.last().unwrap(),
        &temp.path().display().to_string()
    );
}

#[test]
fn strip_ansi_removes_color_sequences() {
    assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m plain"), "red plain");
}
