use std::fs;

use zcode_tui::{
    build_prompt_command, classify_input, load_mcp_config, parse_cli_args, save_mcp_config,
    strip_ansi, InputAction, McpServer,
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
