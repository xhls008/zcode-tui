//! Read-only inspection of official Desktop 3.11.2 / 3.12.3 snapshot state.
//! No kernel, network, credential access, archive extraction, or decryption.
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_JSON: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
pub const HELP: &str = "Usage: zcode-tui check-zhipu-upload [--home PATH] [--data-base-dir PATH] [--json] [--list-files]\nRead-only local evidence check for official ZCode Desktop 3.11.2/3.12.3.\nDoes not start ZCode, contact servers, read file contents from your workspace, or decrypt archives.\nNo evidence is NOT proof that nothing was uploaded. --list-files reveals manifest filenames.\nExit: 0 completed (inspect findings), 1 invalid arguments/fatal error. Coverage warnings are in the report.";

#[derive(Default)]
pub struct Options {
    pub home: PathBuf,
    pub data_base_dir: Option<PathBuf>,
    pub list_files: bool,
    pub json: bool,
}
#[derive(Debug, Serialize)]
pub struct Finding {
    /// Anonymous ordinal; no project/credential identifiers emitted by default.
    pub checkpoint: usize,
    pub accepted_state: bool,
    pub attempted: bool,
    pub staged: bool,
    pub encrypted_archives: usize,
    pub accepted_manifest_files: usize,
    pub accepted_manifests_read: usize,
    pub filenames: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub scope: &'static str,
    pub status: &'static str,
    pub conclusion: &'static str,
    pub roots_checked: usize,
    pub findings: Vec<Finding>,
    pub warnings: Vec<String>,
    pub limitations: &'static str,
}
const LIMITATIONS: &str = "Client accepted-state is evidence of client-reported success, not independent server confirmation. Manifests describe snapshots, not necessarily each incremental upload. Missing/deleted/relocated artifacts, other accounts, logs, other upload mechanisms and server retention are not covered. No evidence does NOT mean never uploaded. 清理、迁移或其他用户的数据不在完整覆盖范围；未发现证据≠从未上传。";

pub fn run(args: &[String]) -> Result<String> {
    let mut opt = Options {
        home: crate::user_home_dir().unwrap_or_default(),
        ..Options::default()
    };
    let explicit_home = args.iter().any(|arg| arg == "--home");
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(HELP.into()),
            "--json" => opt.json = true,
            "--list-files" => opt.list_files = true,
            "--home" => {
                opt.home = PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--home requires PATH"))?,
                )
            }
            "--data-base-dir" => {
                opt.data_base_dir =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        anyhow::anyhow!("--data-base-dir requires PATH")
                    })?))
            }
            _ => bail!("unknown check-zhipu-upload argument (use --help)"),
        }
    }
    if opt.home.as_os_str().is_empty() {
        bail!("home unavailable; specify --home PATH");
    }
    // An explicit --home isolates tests and offline inspection from the caller's env.
    let env_root = if explicit_home {
        None
    } else {
        std::env::var_os("ZCODE_DATA_BASE_DIR").map(PathBuf::from)
    };
    let report = scan(&opt, env_root.as_deref());
    if opt.json {
        return Ok(serde_json::to_string_pretty(&report)?);
    }
    let mut text = format!(
        "check-zhipu-upload — {}\nChecked roots: {}\n",
        report.conclusion, report.roots_checked
    );
    for f in &report.findings {
        text.push_str(&format!("Checkpoint #{}: client accepted / 客户端记录成功={}, attempted / 尝试={}, staged / 已打包或排队={}, encrypted archives={}, readable accepted manifests={}, snapshot files={}\n", f.checkpoint, f.accepted_state, f.attempted, f.staged, f.encrypted_archives, f.accepted_manifests_read, f.accepted_manifest_files));
        for name in &f.filenames {
            text.push_str(&format!("  {name:?}\n"));
        }
    }
    for warning in &report.warnings {
        text.push_str(&format!("Coverage warning: {warning}\n"));
    }
    text.push_str(LIMITATIONS);
    Ok(text)
}

// Reject links at EVERY path component, including an intermediate directory.
// This is a best-effort offline reader, not protection against concurrent hostile
// filesystem replacement; stop Desktop before a forensic scan for stable results.
fn safe_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    let mut current = PathBuf::new();
    for part in path.components() {
        if matches!(part, Component::ParentDir) {
            return Err(std::io::Error::other("parent traversal"));
        }
        current.push(part);
        // A Windows drive/UNC prefix alone (especially canonical \\?\ paths)
        // is not a directory. Inspect it only after the following RootDir.
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(std::io::Error::other("symlink"));
        }
    }
    fs::symlink_metadata(path)
}
fn json(path: &Path, budget: &mut u64) -> Option<Value> {
    let meta = safe_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_JSON || meta.len() > *budget {
        return None;
    }
    *budget -= meta.len();
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_JSON + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_JSON {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
fn present(v: &Value, key: &str) -> bool {
    v.get(key)
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
}
fn warn(report: &mut Report, msg: &str) {
    // Fixed strings only: parse errors and arbitrary state values can leak secrets.
    if !report.warnings.iter().any(|x| x == msg) {
        report.warnings.push(msg.into());
    }
}
fn children(path: &Path, report: &mut Report, budget: &mut usize) -> Vec<PathBuf> {
    match safe_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return vec![],
        Ok(m) if m.is_dir() => {}
        _ => {
            warn(
                report,
                "Unreadable, linked, or non-directory scan path skipped.",
            );
            return vec![];
        }
    }
    let Ok(entries) = fs::read_dir(path) else {
        warn(report, "Directory could not be read.");
        return vec![];
    };
    let mut paths = Vec::new();
    for entry in entries {
        if *budget == 0 {
            warn(report, "Entry limit reached; scan is incomplete.");
            break;
        }
        *budget -= 1;
        match entry {
            Ok(e) => paths.push(e.path()),
            Err(_) => warn(report, "Directory entry could not be read."),
        }
    }
    paths.sort();
    paths
}

pub fn scan(opt: &Options, env_root: Option<&Path>) -> Report {
    let mut report = Report {
        schema_version: 1,
        scope: "official Desktop checkpoint state (3.11.2 / 3.12.3); local only",
        status: "no_local_evidence",
        conclusion: "no_local_evidence / 未发现本地证据",
        roots_checked: 0,
        findings: vec![],
        warnings: vec![],
        limitations: LIMITATIONS,
    };
    if !opt.home.is_absolute() {
        warn(&mut report, "Home must be an absolute path.");
        report.status = "inconclusive";
        report.conclusion = "inconclusive / 覆盖不完整，无法判断";
        return report;
    }
    // Resolve only the explicitly selected base (e.g. macOS /var -> /private/var).
    // Links inside .zcode/checkpoints remain forbidden.
    let mut json_budget = 64 * 1024 * 1024;
    let home = fs::canonicalize(&opt.home).unwrap_or_else(|_| opt.home.clone());
    let mut roots = BTreeSet::from([home.clone()]);
    let setting = home.join(".zcode/v2/setting.json");
    match safe_metadata(&setting) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => match json(&setting, &mut json_budget) {
            Some(v) => {
                if let Some(p) = v.get("dataBaseDir").and_then(Value::as_str).filter(|p| !p.is_empty()) {
                    roots.insert(PathBuf::from(p));
                }
            }
            None => warn(&mut report, "Settings unreadable, linked, oversized, or invalid; configured data directory may be missed."),
        },
    }
    if let Some(p) = env_root {
        roots.insert(p.to_path_buf());
    }
    if let Some(p) = &opt.data_base_dir {
        roots.insert(p.clone());
    }
    let mut budget = MAX_ENTRIES;
    let roots: BTreeSet<_> = roots
        .into_iter()
        .map(|base| {
            if base.is_absolute() {
                fs::canonicalize(&base).unwrap_or(base)
            } else {
                base
            }
        })
        .collect();
    for base in roots {
        if !base.is_absolute() {
            warn(
                &mut report,
                "Relative data directory skipped; supply an absolute path.",
            );
            continue;
        }
        if !safe_metadata(&base).is_ok_and(|m| m.is_dir()) {
            warn(
                &mut report,
                "Selected data root does not exist or cannot be read.",
            );
            continue;
        }
        for name in ["checkpoints", "repo-snapshots"] {
            report.roots_checked += 1;
            let root = base.join(".zcode/v2").join(name);
            for dir in children(&root, &mut report, &mut budget) {
                if !safe_metadata(&dir).is_ok_and(|m| m.is_dir()) {
                    warn(&mut report, "Non-directory or linked checkpoint skipped.");
                    continue;
                }
                let mut f = Finding {
                    checkpoint: report.findings.len() + 1,
                    accepted_state: false,
                    attempted: false,
                    staged: false,
                    encrypted_archives: 0,
                    accepted_manifest_files: 0,
                    accepted_manifests_read: 0,
                    filenames: vec![],
                };
                let state_path = dir.join("state.json");
                if let Some(state) = json(&state_path, &mut json_budget) {
                    if !state.is_object()
                        || !present(&state, "workspaceKey")
                        || !present(&state, "workspacePath")
                    {
                        warn(&mut report, "Unrecognized state schema; no success conclusion drawn from this state.");
                    } else {
                        f.accepted_state = present(&state, "lastAcceptedManifestHash")
                            || present(&state, "lastAcceptedExtraManifestHash");
                        f.attempted = state
                            .get("failureCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(0)
                            > 0;
                        // activeUpload and pendingUpload are aliases in 3.12.3. Booleans cannot double count.
                        for key in ["activeUpload", "pendingUpload", "latestPendingUpload"] {
                            if let Some(p) = state.get(key).filter(|p| p.is_object()) {
                                f.staged = true;
                                f.attempted |=
                                    p.get("attemptCount").and_then(Value::as_u64).unwrap_or(0) > 0
                                        || present(p, "lastAttemptAt")
                                        || present(p, "failureCountedAt");
                            }
                        }
                        for (hash_key, folder, path_key) in [
                            (
                                "lastAcceptedManifestHash",
                                "manifests",
                                "lastAcceptedManifestPath",
                            ),
                            (
                                "lastAcceptedExtraManifestHash",
                                "extra-manifests",
                                "lastAcceptedExtraManifestPath",
                            ),
                        ] {
                            if let Some(hash) = state
                                .get(hash_key)
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                            {
                                // Derive the manifest path ourselves. Never follow state-supplied absolute references.
                                if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit())
                                {
                                    warn(&mut report, "Accepted state has an unrecognized manifest hash; manifest not followed.");
                                    continue;
                                }
                                let path = dir.join(folder).join(format!("{hash}.json"));
                                if state
                                    .get(path_key)
                                    .and_then(Value::as_str)
                                    .is_some_and(|p| Path::new(p) != path)
                                {
                                    warn(&mut report, "State manifest reference differs from safe checkpoint path; external reference not followed.");
                                }
                                if let Some(m) = json(&path, &mut json_budget)
                                    .filter(|m| m.get("workspaceKey") == state.get("workspaceKey"))
                                {
                                    if let Some(files) = m.get("files").and_then(Value::as_array) {
                                        f.accepted_manifests_read += 1;
                                        f.accepted_manifest_files += files.len();
                                        if opt.list_files {
                                            for file in files {
                                                if let Some(path) =
                                                    file.get("path").and_then(Value::as_str)
                                                {
                                                    f.filenames.push(path.into());
                                                }
                                            }
                                        }
                                    } else {
                                        warn(&mut report, "Accepted manifest schema unrecognized; contents not inventoried.");
                                    }
                                } else {
                                    warn(&mut report, "Accepted manifest missing, mismatched, unreadable, linked, oversized, or invalid. File inventory is incomplete.");
                                }
                            }
                        }
                    }
                } else if !matches!(safe_metadata(&state_path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
                {
                    warn(
                        &mut report,
                        "State unreadable, linked, oversized, or invalid; results incomplete.",
                    );
                }
                for folder in ["pending", "tmp"] {
                    for path in children(&dir.join(folder), &mut report, &mut budget) {
                        if !safe_metadata(&path).is_ok_and(|m| m.is_file()) {
                            warn(&mut report, "Linked or non-file artifact skipped.");
                            continue;
                        }
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if name.ends_with(".tar.gz.enc") {
                            f.encrypted_archives += 1;
                            f.staged = true;
                        }
                        if name.ends_with(".envelope.json") || name.ends_with(".tar.gz") {
                            f.staged = true;
                        }
                    }
                }
                if f.accepted_state || f.attempted || f.staged {
                    report.findings.push(f);
                }
            }
        }
    }
    report.conclusion = if report.findings.iter().any(|f| f.accepted_state) {
        "client_reported_accepted / 发现客户端记录成功的证据"
    } else if report.findings.iter().any(|f| f.attempted) {
        "attempt_evidence / 发现尝试证据，不代表成功"
    } else if !report.findings.is_empty() {
        "staged_evidence / 发现打包或排队证据，不代表上传"
    } else if !report.warnings.is_empty() {
        "inconclusive / 覆盖不完整，无法判断"
    } else {
        "no_local_evidence / 未发现本地证据"
    };
    report.status = report
        .conclusion
        .split(" / ")
        .next()
        .unwrap_or("inconclusive");
    report
}
