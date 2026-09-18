use serde_json::json;
use std::{fs, path::Path, process::Command};
use zcode_tui::{
    classify_input,
    upload_check::{scan, Options},
    InputAction,
};

fn put(path: &Path, value: serde_json::Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value.to_string()).unwrap();
}
fn options(home: &Path) -> Options {
    Options {
        home: home.into(),
        ..Default::default()
    }
}
fn checkpoint(home: &Path) -> std::path::PathBuf {
    home.join(".zcode/v2/checkpoints/123456abcdef")
}

#[test]
fn no_evidence_is_not_a_clean_bill_and_scan_creates_nothing() {
    let t = tempfile::tempdir().unwrap();
    let r = scan(&options(t.path()), None);
    assert!(r.conclusion.starts_with("no_local_evidence"));
    assert!(r.limitations.contains("NOT mean never uploaded"));
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 0);
}
#[test]
fn accepted_pending_and_failed_coexist_without_exposing_credentials() {
    let t = tempfile::tempdir().unwrap();
    let d = checkpoint(t.path());
    let h = "a".repeat(64);
    let pending = json!({"groupId":"secret-group", "uploadCredentialHandle":"DO_NOT_PRINT", "attemptCount":3});
    let state = json!({"workspaceKey":"private-key", "workspacePath":"private-project", "lastAcceptedManifestHash":h, "activeUpload":pending, "pendingUpload":pending, "latestPendingUpload":{"attemptCount":0}});
    put(&d.join("state.json"), state.clone());
    put(
        &d.join(format!("manifests/{h}.json")),
        json!({"workspaceKey":"private-key","files":[{"path":".git/config","sizeBytes":2},{"path":"secret-name.txt","sizeBytes":9}]}),
    );
    fs::create_dir_all(d.join("pending")).unwrap();
    fs::write(d.join("pending/one.tar.gz.enc"), "not-real-ciphertext").unwrap();
    let r = scan(&options(t.path()), None);
    let f = &r.findings[0];
    assert!(f.accepted_state && f.attempted && f.staged);
    assert_eq!(f.encrypted_archives, 1);
    assert_eq!(f.accepted_manifest_files, 2);
    let text = serde_json::to_string(&r).unwrap();
    for secret in [
        "DO_NOT_PRINT",
        "private-key",
        "private-project",
        "secret-name",
        "secret-group",
    ] {
        assert!(!text.contains(secret));
    }
    let mut opt = options(t.path());
    opt.list_files = true;
    assert_eq!(scan(&opt, None).findings[0].filenames.len(), 2);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(d.join("state.json")).unwrap()
        )
        .unwrap(),
        state
    );
}
#[test]
fn attempts_and_archives_are_not_success() {
    let t = tempfile::tempdir().unwrap();
    let d = checkpoint(t.path());
    put(
        &d.join("state.json"),
        json!({"workspaceKey":"k","workspacePath":"p","pendingUpload":{"attemptCount":2},"failureCount":2}),
    );
    let r = scan(&options(t.path()), None);
    assert!(r.conclusion.starts_with("attempt_evidence"));
    assert!(!r.findings[0].accepted_state);
    fs::remove_file(d.join("state.json")).unwrap();
    fs::create_dir_all(d.join("pending")).unwrap();
    fs::write(d.join("pending/a.tar.gz.enc"), []).unwrap();
    assert!(scan(&options(t.path()), None)
        .conclusion
        .starts_with("staged_evidence"));
}
#[test]
fn configured_and_legacy_roots_and_explicit_root_are_scanned() {
    let t = tempfile::tempdir().unwrap();
    let alt = tempfile::tempdir().unwrap();
    put(
        &t.path().join(".zcode/v2/setting.json"),
        json!({"dataBaseDir":alt.path()}),
    );
    put(
        &alt.path().join(".zcode/v2/repo-snapshots/abc/state.json"),
        json!({"workspaceKey":"k","workspacePath":"p","failureCount":2}),
    );
    assert_eq!(scan(&options(t.path()), None).findings.len(), 1);
    fs::remove_file(t.path().join(".zcode/v2/setting.json")).unwrap();
    assert_eq!(scan(&options(t.path()), Some(alt.path())).findings.len(), 1);
    let mut o = options(t.path());
    o.data_base_dir = Some(alt.path().into());
    assert_eq!(scan(&o, None).findings.len(), 1);
}
#[test]
fn corrupted_unknown_and_oversized_states_warn_without_raw_data() {
    let t = tempfile::tempdir().unwrap();
    let p = checkpoint(t.path()).join("state.json");
    put(&p, json!({"lastAcceptedManifestHash":"fake"}));
    assert!(!scan(&options(t.path()), None).warnings.is_empty());
    fs::write(&p, "private-invalid-json").unwrap();
    let r = scan(&options(t.path()), None);
    assert!(r.conclusion.starts_with("inconclusive"));
    assert!(!serde_json::to_string(&r)
        .unwrap()
        .contains("private-invalid"));
    fs::File::create(&p)
        .unwrap()
        .set_len(9 * 1024 * 1024)
        .unwrap();
    assert!(!scan(&options(t.path()), None).warnings.is_empty());
}
#[test]
fn accepted_state_survives_missing_manifest_but_does_not_follow_external_path() {
    let t = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let p = external.path().join("manifest.json");
    put(
        &p,
        json!({"workspaceKey":"k","files":[{"path":"SHOULD_NOT_READ"}]}),
    );
    put(
        &checkpoint(t.path()).join("state.json"),
        json!({"workspaceKey":"k","workspacePath":"p","lastAcceptedManifestHash":"b".repeat(64),"lastAcceptedManifestPath":p}),
    );
    let mut o = options(t.path());
    o.list_files = true;
    let r = scan(&o, None);
    assert!(r.findings[0].accepted_state);
    assert!(r.findings[0].filenames.is_empty());
    assert!(!r.warnings.is_empty());
}
#[cfg(unix)]
#[test]
fn symlinked_roots_and_manifests_are_not_followed() {
    let t = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    let d = checkpoint(e.path());
    put(
        &d.join("state.json"),
        json!({"workspaceKey":"k","workspacePath":"p","failureCount":2}),
    );
    std::os::unix::fs::symlink(e.path().join(".zcode"), t.path().join(".zcode")).unwrap();
    let r = scan(&options(t.path()), None);
    assert!(r.findings.is_empty());
    assert!(!r.warnings.is_empty());
}
#[test]
fn command_is_local_and_does_not_start_kernel_or_create_config() {
    assert!(matches!(
        classify_input("/check-zhipu-upload --json").unwrap(),
        InputAction::Local(_)
    ));
    let t = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_zcode-tui"))
        .args([
            "check-zhipu-upload",
            "--home",
            t.path().to_str().unwrap(),
            "--json",
        ])
        .env("ZCODE_TUI_ZCODE_BIN", "/must-not-run")
        .output()
        .unwrap();
    assert!(output.status.success());
    let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(r["schema_version"], 1);
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 0);
}

#[test]
fn missing_or_relative_roots_are_inconclusive_not_clean() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(
        scan(&options(&t.path().join("missing")), None).status,
        "inconclusive"
    );
    assert_eq!(
        scan(&options(Path::new("relative")), None).status,
        "inconclusive"
    );
}

#[cfg(unix)]
#[test]
fn linked_manifest_and_parent_traversal_hash_never_reveal_external_files() {
    let t = tempfile::tempdir().unwrap();
    let e = tempfile::tempdir().unwrap();
    let d = checkpoint(t.path());
    let h = "c".repeat(64);
    put(
        &d.join("state.json"),
        json!({"workspaceKey":"k","workspacePath":"p","lastAcceptedManifestHash":h}),
    );
    let manifest = e.path().join("outside.json");
    put(
        &manifest,
        json!({"workspaceKey":"k","files":[{"path":"EXTERNAL_SECRET"}]}),
    );
    fs::create_dir_all(d.join("manifests")).unwrap();
    std::os::unix::fs::symlink(&manifest, d.join(format!("manifests/{h}.json"))).unwrap();
    let mut opt = options(t.path());
    opt.list_files = true;
    let r = scan(&opt, None);
    assert!(r.findings[0].filenames.is_empty());
    assert!(!r.warnings.is_empty());
    put(
        &d.join("state.json"),
        json!({"workspaceKey":"k","workspacePath":"p","lastAcceptedManifestHash":"../../outside"}),
    );
    let r = scan(&opt, None);
    assert!(r.findings[0].filenames.is_empty());
    assert!(!r.warnings.is_empty());
}
