//! Opt-in, credential-free Linux/macOS kernel compatibility check.
//! ZCODE_TEST_CJS=/path/to/resources/glm/zcode.cjs cargo test --test official_kernel -- --ignored --nocapture
//! Model traffic goes only to a local fake provider, never a paid service.

#![cfg(unix)]

use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use zcode_tui::{
    app_close_params, app_create_params, app_resume_params, app_send_params,
    app_session_id_from_result, app_session_read_params, app_state_is_turn_end,
    app_state_turn_error, app_subscribe_params, build_runtime_model,
    encode_official_mcp_auth_headers_reply, encode_runtime_preferences_reply,
    v4_conversation_subscribe_params, with_runtime_model, AppServerConn, AppServerMessage,
};

fn poll(conn: &mut AppServerConn, preferences: &mut usize) -> Option<AppServerMessage> {
    match conn.poll() {
        Some(AppServerMessage::ServerRequest { id, method, .. }) => {
            let reply = encode_runtime_preferences_reply(&id, &method)
                .inspect(|_| {
                    *preferences += 1;
                })
                .or_else(|| encode_official_mcp_auth_headers_reply(&id, &method));
            conn.reply(&reply.unwrap_or_else(|| panic!("unexpected server request: {method}")))
                .unwrap();
            None
        }
        message => message,
    }
}

fn request(
    conn: &mut AppServerConn,
    method: &str,
    params: Value,
    preferences: &mut usize,
) -> Value {
    let want = conn.send(method, params).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(Instant::now() < deadline, "{method} timed out");
        match poll(conn, preferences) {
            Some(AppServerMessage::Response { id, result, error }) if id == want => {
                assert!(error.is_none(), "{method}: {error:?}");
                println!("{method}: passed");
                return result.expect("response result");
            }
            None => {
                assert!(conn.is_alive(), "{method}: disconnected");
                thread::sleep(Duration::from_millis(10));
            }
            _ => {}
        }
    }
}

#[test]
#[ignore = "requires an explicitly supplied official kernel; uses a temporary HOME"]
fn official_kernel_session_lifecycle() {
    let kernel = fs::canonicalize(std::env::var("ZCODE_TEST_CJS").expect("set ZCODE_TEST_CJS"))
        .expect("kernel file");
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().to_str().unwrap();
    // One test-only loopback HTTP/SSE endpoint, using the standard library.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let server = thread::spawn(move || {
        'requests: while !server_stop.load(Ordering::Relaxed) {
            let (mut socket, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("fake provider: {error}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut length = 0;
            loop {
                let mut line = String::new();
                // A cancelled request may close a preconnected socket before headers.
                if !matches!(reader.read_line(&mut line), Ok(n) if n > 0) {
                    continue 'requests;
                }
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            assert!(
                length > 0 && length < 2_000_000,
                "unexpected request body size"
            );
            let mut body = vec![0; length];
            if reader.read_exact(&mut body).is_err() {
                continue 'requests;
            }
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["model"], "compat-model");
            let streaming = body["stream"] == true;
            let message = json!({"id":"msg_compat", "type":"message", "role":"assistant",
                "model":"compat-model", "content":[], "stop_reason":null,
                "stop_sequence":null, "usage":{"input_tokens":10,"output_tokens":0}});
            let events = [
                json!({"type":"message_start","message":message}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"compatibility-ok"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}),
                json!({"type":"message_stop"}),
            ];
            let body: String = if streaming {
                events
                    .iter()
                    .map(|event| {
                        format!(
                            "event: {}\ndata: {event}\n\n",
                            event["type"].as_str().unwrap()
                        )
                    })
                    .collect()
            } else {
                json!({"id":"msg_compat", "type":"message", "role":"assistant",
                    "model":"compat-model", "content":[{"type":"text","text":"compatibility-ok"}],
                    "stop_reason":"end_turn", "stop_sequence":null,
                    "usage":{"input_tokens":10,"output_tokens":3}})
                .to_string()
            };
            let content_type = if streaming {
                "text/event-stream"
            } else {
                "application/json"
            };
            thread::sleep(Duration::from_millis(200));
            // The cancellation scenario deliberately closes an in-flight request.
            let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    let config = json!({
        "provider": {"compat": {"kind": "anthropic", "name": "Offline compatibility check",
            "options": {"baseURL": base_url, "apiKey": "not-a-real-key"},
            "models": {"compat-model": {"name": "Offline model"}}}},
        "model": {"main": "compat/compat-model", "lite": "compat/compat-model"}
    })
    .to_string();
    fs::create_dir_all(temp.path().join(".zcode/cli")).unwrap();
    fs::write(temp.path().join(".zcode/cli/config.json"), &config).unwrap();
    let runtime = build_runtime_model(&config, 1).unwrap();
    let wrapper = temp.path().join("kernel");
    // Sanitize only the child environment; do not mutate the test runner's HOME.
    let quote = shell_words::quote;
    fs::write(&wrapper, format!(
        "#!/bin/sh\ncd {home} || exit 1\nexec env -i HOME={home} XDG_CONFIG_HOME={home}/.config XDG_DATA_HOME={home}/.local/share XDG_CACHE_HOME={home}/.cache PATH={path} node {kernel} \"$@\"\n",
        home=quote(home), path=quote(&std::env::var("PATH").unwrap()),
        kernel=quote(kernel.to_str().unwrap()),
    )).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let mut conn = AppServerConn::spawn(wrapper.to_str().unwrap()).unwrap();
    let mut preferences = 0;
    let created = request(
        &mut conn,
        "session/create",
        with_runtime_model(app_create_params(home), Some(&runtime)),
        &mut preferences,
    );
    let session = app_session_id_from_result(&created).expect("created session ID");
    assert!(preferences > 0, "runtime preferences were not exercised");
    let subscribed = request(
        &mut conn,
        "session/subscribe",
        app_subscribe_params(&session),
        &mut preferences,
    );
    assert_eq!(subscribed["sessionId"], session);
    let read = request(
        &mut conn,
        "session/read",
        app_session_read_params(&session),
        &mut preferences,
    );
    assert_eq!(
        app_session_id_from_result(&read).as_deref(),
        Some(session.as_str())
    );
    let v4 = request(
        &mut conn,
        "v4/conversation/subscribe",
        v4_conversation_subscribe_params(&session, "compat-smoke"),
        &mut preferences,
    );
    assert!(v4
        .pointer("/ack/subscriptionId")
        .and_then(Value::as_str)
        .is_some());
    // A newly created session is not persisted until its first turn. Exercise a
    // real stream before testing DB-backed subagent listing and close/resume.
    run_turn(&mut conn, &session, &mut preferences);
    // The second turn starts on the same session, not a fresh create.
    run_turn(&mut conn, &session, &mut preferences);
    conn.send(
        "session/send",
        app_send_params(&session, "Cancellation check"),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(Instant::now() < deadline, "cancellation turn never started");
        if matches!(poll(&mut conn, &mut preferences), Some(AppServerMessage::Event(event)) if event.kind == "turn.started")
        {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let stop_id = conn
        .send(
            "v4/command",
            zcode_tui::v4_command_params(
                "compat-stop",
                "compat-smoke",
                &session,
                "stop",
                json!({}),
                zcode_tui::V4CommandBase::None,
                1,
            ),
        )
        .unwrap();
    let (mut ack, mut terminal) = (false, false);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !(ack && terminal) {
        assert!(Instant::now() < deadline, "stop did not settle");
        match poll(&mut conn, &mut preferences) {
            Some(AppServerMessage::Response { id, result, error }) if id == stop_id => {
                assert!(error.is_none(), "V4 stop: {error:?}");
                assert!(zcode_tui::parse_v4_command_ack(&result.unwrap())
                    .unwrap()
                    .accepted());
                ack = true;
            }
            Some(AppServerMessage::Event(event))
                if matches!(event.kind.as_str(), "turn.failed" | "turn.completed") =>
            {
                terminal = true
            }
            None => thread::sleep(Duration::from_millis(10)),
            _ => {}
        }
    }
    println!("V4 foreground cancellation: passed");
    // As in the UI, discard the cancelled turn's tail before reusing the session.
    while conn.poll().is_some() {}
    run_turn(&mut conn, &session, &mut preferences);
    request(
        &mut conn,
        "session/subagents",
        json!({"sessionId": session}),
        &mut preferences,
    );
    request(
        &mut conn,
        "session/usage",
        json!({"sessionId": session}),
        &mut preferences,
    );
    request(
        &mut conn,
        "session/close",
        app_close_params(&session),
        &mut preferences,
    );
    let resumed = request(
        &mut conn,
        "session/resume",
        app_resume_params(&session, Some(&runtime)),
        &mut preferences,
    );
    assert_eq!(
        app_session_id_from_result(&resumed).as_deref(),
        Some(session.as_str())
    );
    request(
        &mut conn,
        "session/close",
        app_close_params(&session),
        &mut preferences,
    );
    println!("runtime-preferences replies: {preferences}");
    drop(conn);
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
}

fn run_turn(conn: &mut AppServerConn, session: &str, preferences: &mut usize) {
    let send = conn
        .send(
            "session/send",
            app_send_params(session, "Reply with compatibility-ok; do not use tools."),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let (mut accepted, mut done, mut frames) = (false, false, 0);
    let mut turn = zcode_tui::AppServerTurn::default();
    while !(accepted && done) {
        assert!(
            Instant::now() < deadline,
            "stream timed out; text={:?}",
            turn.text
        );
        match poll(conn, preferences) {
            Some(AppServerMessage::Response { id, result, error }) if id == send => {
                assert!(error.is_none(), "session/send: {error:?}");
                assert_eq!(result.unwrap()["accepted"], true);
                accepted = true;
            }
            Some(AppServerMessage::Event(event)) => {
                assert_ne!(event.kind, "turn.failed", "model turn failed");
                done |= turn.apply(&event) == zcode_tui::TurnDelta::Done;
            }
            Some(AppServerMessage::StateUpdated(state)) => {
                assert!(
                    app_state_turn_error(&state).is_none(),
                    "turn failed: {state}"
                );
                done |= app_state_is_turn_end(&state);
            }
            Some(AppServerMessage::V4Frame(_)) => frames += 1,
            None => thread::sleep(Duration::from_millis(10)),
            _ => {}
        }
    }
    assert_eq!(turn.text, "compatibility-ok");
    assert!(frames > 0, "missing V4 frames during the turn");
    println!("local-provider streaming + V4 frames + turn completion: passed");
}
