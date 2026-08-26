#!/usr/bin/env python3
"""Deterministic state and quality gates for the Codex Feature Workflow."""

from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import json
import os
import re
import shutil
import sys
from pathlib import Path
from typing import Any, Iterator

try:
    import fcntl  # type: ignore[import-not-found]
except ImportError:  # Windows
    fcntl = None  # type: ignore[assignment]


SCHEMA = "feature-workflow/v1"
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
DEEP_SIGNALS = [
    "multi_module",
    "public_api",
    "database_migration",
    "parallel_split",
    "external_dependency",
    "authorization",
    "data_consistency",
    "compatibility",
]


class WorkflowError(RuntimeError):
    pass


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def read_json(path: Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as handle:
            value = json.load(handle)
    except FileNotFoundError as exc:
        raise WorkflowError(f"required file is missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise WorkflowError(f"invalid JSON in {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise WorkflowError(f"expected a JSON object in {path}")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp.{os.getpid()}")
    with temporary.open("w", encoding="utf-8") as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


@contextlib.contextmanager
def state_lock(root: Path) -> Iterator[None]:
    lock_path = root / "feature-workflow" / ".state.lock"
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    with lock_path.open("a+", encoding="utf-8") as handle:
        if fcntl is not None:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
        else:
            import msvcrt

            handle.seek(0, os.SEEK_END)
            if handle.tell() == 0:
                handle.write("0")
                handle.flush()
            handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)
        try:
            yield
        finally:
            if fcntl is not None:
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
            else:
                import msvcrt

                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)


def paths(root: Path) -> tuple[Path, Path, Path]:
    return (
        root / "feature-workflow" / "config.json",
        root / "feature-workflow" / "queue.json",
        root / "features" / "archive" / "archive.json",
    )


def require_initialized(root: Path) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    config_path, queue_path, archive_path = paths(root)
    return read_json(config_path), read_json(queue_path), read_json(archive_path)


def parse_csv(value: str | None) -> list[str]:
    if not value:
        return []
    return list(dict.fromkeys(item.strip() for item in value.split(",") if item.strip()))


def validate_id(feature_id: str) -> None:
    if not ID_RE.fullmatch(feature_id):
        raise WorkflowError(f"invalid feature ID: {feature_id!r}")


def all_entries(queue: dict[str, Any]) -> Iterator[tuple[str, dict[str, Any]]]:
    for section in ("parents", "active", "pending", "blocked", "completed"):
        for entry in queue.get(section, []):
            yield section, entry


def find_entry(queue: dict[str, Any], feature_id: str) -> tuple[str, dict[str, Any]]:
    for section, entry in all_entries(queue):
        if entry.get("id") == feature_id:
            return section, entry
    raise WorkflowError(f"feature not found in queue: {feature_id}")


def feature_dir(root: Path, state: str, feature_id: str) -> Path:
    return root / "features" / f"{state}-{feature_id}"


def command_init(args: argparse.Namespace) -> None:
    root = args.root
    config_path, queue_path, archive_path = paths(root)
    existing = [str(path) for path in (config_path, queue_path, archive_path) if path.exists()]
    if existing:
        raise WorkflowError("workflow already initialized; refusing to overwrite: " + ", ".join(existing))

    main_branch = args.main_branch
    project_name = args.project_name or root.name
    config = {
        "schema": SCHEMA,
        "project": {"name": project_name, "main_branch": main_branch, "repo_path": "."},
        "parallelism": {"max_concurrent": args.max_concurrent},
        "workflow": {
            "routing": {"default": "lite", "deep_signals": DEEP_SIGNALS, "allow_manual_override": True},
            "review": {"required_for": ["deep"], "min_score": 70},
            "require_checklist": True,
            "require_verification": True,
        },
        "git": {
            "remote": "origin",
            "auto_push": False,
            "push_tags": False,
            "branch_prefix": "feature",
            "worktree_base": "..",
        },
        "completion": {"create_tag": True, "delete_worktree": True, "delete_branch": True},
    }
    now = utc_now()
    queue = {"schema": SCHEMA, "meta": {"last_updated": now}, "parents": [], "active": [], "pending": [], "blocked": [], "completed": []}
    archive = {"schema": "feature-archive/v1", "meta": {"last_updated": now, "total_completed": 0}, "records": []}
    write_json(config_path, config)
    write_json(queue_path, queue)
    write_json(archive_path, archive)

    runtime_dir = root / "feature-workflow" / "scripts"
    runtime_dir.mkdir(parents=True, exist_ok=True)
    target = runtime_dir / "workflow.py"
    source = Path(__file__).resolve()
    if source != target.resolve():
        shutil.copy2(source, target)
    print(json.dumps({"status": "initialized", "root": str(root), "project": project_name}, ensure_ascii=False))


def command_create(args: argparse.Namespace) -> None:
    validate_id(args.id)
    root = args.root
    signals = parse_csv(args.signals)
    dependencies = parse_csv(args.dependencies)
    for dependency in dependencies:
        validate_id(dependency)

    with state_lock(root):
        config, queue, _archive = require_initialized(root)
        if any(entry.get("id") == args.id for _section, entry in all_entries(queue)):
            raise WorkflowError(f"feature ID already exists: {args.id}")

        configured_signals = set(config["workflow"]["routing"].get("deep_signals", DEEP_SIGNALS))
        mode = args.mode
        if mode == "auto":
            mode = "deep" if configured_signals.intersection(signals) else config["workflow"]["routing"].get("default", "lite")

        directory = feature_dir(root, "pending", args.id)
        if directory.exists():
            raise WorkflowError(f"feature directory already exists: {directory}")
        directory.mkdir(parents=True)
        now = utc_now()
        spec = f"""# Feature: {args.id} {args.name}\n\n## Basic information\n- ID: {args.id}\n- Priority: {args.priority}\n- Workflow mode: {mode}\n- Risk signals: {', '.join(signals) or 'none'}\n- Dependencies: {', '.join(dependencies) or 'none'}\n- Created: {now}\n\n## User outcome\n{args.description}\n\n## Scope and constraints\n\n## Acceptance scenarios\n\n## Technical notes\n"""
        task = f"# Tasks: {args.id}\n\n- [ ] Define the implementation plan from repository context\n\n## Progress log\n"
        checklist = f"# Checklist: {args.id}\n\n- [ ] All planned tasks completed\n- [ ] Required quality checks passed\n- [ ] Tests passed\n- [ ] Acceptance scenarios passed\n- [ ] Verification evidence saved\n"
        (directory / "spec.md").write_text(spec, encoding="utf-8")
        (directory / "task.md").write_text(task, encoding="utf-8")
        (directory / "checklist.md").write_text(checklist, encoding="utf-8")

        entry = {
            "id": args.id,
            "name": args.name,
            "description": args.description,
            "priority": args.priority,
            "workflow_mode": mode,
            "risk_signals": signals,
            "dependencies": dependencies,
            "status": "pending",
            "created_at": now,
        }
        queue["pending"].append(entry)
        queue["pending"].sort(key=lambda item: (-int(item.get("priority", 0)), item["id"]))
        queue["meta"]["last_updated"] = now
        write_json(paths(root)[1], queue)
    print(json.dumps({"status": "created", "id": args.id, "workflow_mode": mode, "risk_signals": signals}, ensure_ascii=False))


def validate_deep_review(root: Path, config: dict[str, Any], entry: dict[str, Any]) -> None:
    mode = entry.get("workflow_mode", "lite")
    required = config["workflow"].get("review", {}).get("required_for", ["deep"])
    if mode not in required:
        return
    path = feature_dir(root, "pending", entry["id"]) / "review-status.json"
    review = read_json(path)
    minimum = int(config["workflow"]["review"].get("min_score", 70))
    if review.get("schema") != "feature-review/v1" or review.get("feature_id") != entry["id"]:
        raise WorkflowError(f"invalid review status: {path}")
    if review.get("status") != "passed" or int(review.get("score", -1)) < minimum or review.get("blocking_issues"):
        raise WorkflowError(f"Deep review gate failed for {entry['id']}")


def command_start_state(args: argparse.Namespace) -> None:
    validate_id(args.id)
    root = args.root
    with state_lock(root):
        config, queue, _archive = require_initialized(root)
        section, entry = find_entry(queue, args.id)
        if section != "pending":
            raise WorkflowError(f"feature must be pending, found in {section}")
        completed_ids = {item["id"] for item in queue.get("completed", [])}
        missing = [item for item in entry.get("dependencies", []) if item not in completed_ids]
        if missing:
            raise WorkflowError("unsatisfied dependencies: " + ", ".join(missing))
        if len(queue.get("active", [])) >= int(config["parallelism"]["max_concurrent"]):
            raise WorkflowError("parallelism limit reached")
        validate_deep_review(root, config, entry)

        pending_dir = feature_dir(root, "pending", args.id)
        active_dir = feature_dir(root, "active", args.id)
        if not pending_dir.is_dir() or active_dir.exists():
            raise WorkflowError("feature document directories are inconsistent")
        pending_dir.rename(active_dir)
        queue["pending"] = [item for item in queue["pending"] if item["id"] != args.id]
        entry.update({"status": "active", "branch": args.branch, "worktree": args.worktree, "started_at": utc_now()})
        queue["active"].append(entry)
        queue["meta"]["last_updated"] = utc_now()
        write_json(paths(root)[1], queue)
    print(json.dumps({"status": "active", "id": args.id, "branch": args.branch, "worktree": args.worktree}, ensure_ascii=False))


def validate_completion(root: Path, feature_id: str, skip_checklist: bool = False) -> dict[str, Any]:
    validate_id(feature_id)
    config, queue, _archive = require_initialized(root)
    section, _entry = find_entry(queue, feature_id)
    if section != "active":
        raise WorkflowError(f"feature must be active, found in {section}")
    directory = feature_dir(root, "active", feature_id)
    for name in ("spec.md", "task.md"):
        path = directory / name
        if not path.is_file() or path.stat().st_size == 0:
            raise WorkflowError(f"required feature document missing: {path}")
    report = directory / "evidence" / "verification-report.md"
    status_path = directory / "evidence" / "verification-status.json"
    if not report.is_file() or report.stat().st_size == 0:
        raise WorkflowError(f"verification report missing: {report}")
    status = read_json(status_path)
    valid = (
        status.get("schema") == "feature-verification/v1"
        and status.get("feature_id") == feature_id
        and status.get("status") == "passed"
        and status.get("blocking_failures") == []
        and status.get("tasks", {}).get("incomplete") == 0
        and status.get("quality", {}).get("failed") == 0
        and status.get("tests", {}).get("failed") == 0
        and status.get("scenarios", {}).get("failed") == 0
    )
    if not valid:
        raise WorkflowError(f"verification is not a clean pass: {status_path}")
    if config["workflow"].get("require_checklist", True) and not skip_checklist:
        checklist = directory / "checklist.md"
        if not checklist.is_file() or re.search(r"^\s*[-*]\s+\[\s\]", checklist.read_text(encoding="utf-8"), re.MULTILINE):
            raise WorkflowError(f"checklist is incomplete: {checklist}")
    return status


def command_validate_completion(args: argparse.Namespace) -> None:
    status = validate_completion(args.root, args.id, args.skip_checklist)
    print(json.dumps({"status": "completion_gate_passed", "id": args.id, "verified_at": status.get("verified_at")}, ensure_ascii=False))


def command_needs_attention(args: argparse.Namespace) -> None:
    validate_id(args.id)
    with state_lock(args.root):
        _config, queue, _archive = require_initialized(args.root)
        section, entry = find_entry(queue, args.id)
        if section != "active":
            raise WorkflowError(f"feature must be active, found in {section}")
        entry["status"] = "needs_attention"
        entry["attention_reason"] = args.reason
        entry["updated_at"] = utc_now()
        queue["meta"]["last_updated"] = utc_now()
        write_json(paths(args.root)[1], queue)
    print(json.dumps({"status": "needs_attention", "id": args.id, "reason": args.reason}, ensure_ascii=False))


def command_move(args: argparse.Namespace, destination: str) -> None:
    validate_id(args.id)
    with state_lock(args.root):
        _config, queue, _archive = require_initialized(args.root)
        section, entry = find_entry(queue, args.id)
        if destination == "blocked":
            if section not in ("pending", "active"):
                raise WorkflowError(f"cannot block feature from {section}")
            queue[section] = [item for item in queue[section] if item["id"] != args.id]
            entry.update({"status": "blocked", "blocked_reason": args.reason, "blocked_at": utc_now(), "previous_state": section})
            queue["blocked"].append(entry)
        else:
            if section != "blocked":
                raise WorkflowError(f"feature must be blocked, found in {section}")
            queue["blocked"] = [item for item in queue["blocked"] if item["id"] != args.id]
            prior = entry.pop("previous_state", "pending")
            if prior != "pending":
                raise WorkflowError("active blocked features require Git reconciliation before unblocking")
            entry.pop("blocked_reason", None)
            entry.pop("blocked_at", None)
            entry["status"] = "pending"
            queue["pending"].append(entry)
            queue["pending"].sort(key=lambda item: (-int(item.get("priority", 0)), item["id"]))
        queue["meta"]["last_updated"] = utc_now()
        write_json(paths(args.root)[1], queue)
    print(json.dumps({"status": destination, "id": args.id}, ensure_ascii=False))


def command_complete_state(args: argparse.Namespace) -> None:
    validate_id(args.id)
    archive_dir = (args.root / args.archive_path).resolve()
    try:
        archive_path = archive_dir.relative_to(args.root.resolve()).as_posix()
    except ValueError as exc:
        raise WorkflowError("archive path must stay inside the project root") from exc
    archive_status_path = archive_dir / "evidence" / "verification-status.json"
    archive_report_path = archive_dir / "evidence" / "verification-report.md"
    archived_report = archive_report_path.relative_to(args.root.resolve()).as_posix()
    required = [
        archive_dir / "spec.md",
        archive_dir / "task.md",
        archive_dir / "checklist.md",
        archive_status_path,
        archive_report_path,
    ]
    if any(not path.is_file() or path.stat().st_size == 0 for path in required):
        raise WorkflowError("archive is incomplete: " + str(archive_dir))

    _config, initial_queue, _initial_archive = require_initialized(args.root)
    initial_section, _initial_entry = find_entry(initial_queue, args.id)
    if initial_section == "completed":
        archived_status = read_json(archive_status_path)
        archived_status["report"] = archived_report
        write_json(archive_status_path, archived_status)
        with state_lock(args.root):
            _config, queue, archive = require_initialized(args.root)
            section, entry = find_entry(queue, args.id)
            if section != "completed":
                raise WorkflowError("feature state changed while reconciling completion")
            entry.update(
                {
                    "archive_path": archive_path,
                    "tag": args.tag,
                    "merge_commit": args.merge_commit,
                    "verification": archived_status,
                }
            )
            records = [item for item in archive.get("records", []) if item.get("id") != args.id]
            records.append(entry)
            archive["records"] = records
            archive["meta"].update({"last_updated": utc_now(), "total_completed": len(records)})
            queue["meta"]["last_updated"] = utc_now()
            write_json(paths(args.root)[2], archive)
            write_json(paths(args.root)[1], queue)
        print(json.dumps({"status": "completed", "id": args.id, "idempotent": True}, ensure_ascii=False))
        return
    if initial_section != "active":
        raise WorkflowError(f"feature must be active or completed, found in {initial_section}")

    status = validate_completion(args.root, args.id, args.skip_checklist)
    archived_status = dict(status)
    archived_status["report"] = archived_report
    write_json(archive_status_path, archived_status)

    with state_lock(args.root):
        _config, queue, archive = require_initialized(args.root)
        section, entry = find_entry(queue, args.id)
        if section != "active":
            raise WorkflowError(f"feature must be active, found in {section}")
        now = utc_now()
        completed = {
            "id": args.id,
            "name": entry["name"],
            "status": "completed",
            "completed_at": now,
            "archive_path": archive_path,
            "tag": args.tag,
            "merge_commit": args.merge_commit,
            "workflow_mode": entry.get("workflow_mode", "lite"),
            "verification": archived_status,
        }
        queue["active"] = [item for item in queue["active"] if item["id"] != args.id]
        queue["completed"] = [item for item in queue["completed"] if item["id"] != args.id] + [completed]
        queue["meta"]["last_updated"] = now
        records = [item for item in archive.get("records", []) if item.get("id") != args.id]
        records.append(completed)
        archive["records"] = records
        archive["meta"].update({"last_updated": now, "total_completed": len(records)})
        write_json(paths(args.root)[2], archive)
        write_json(paths(args.root)[1], queue)
    print(json.dumps({"status": "completed", "id": args.id, "archive_path": archive_path, "tag": args.tag}, ensure_ascii=False))


def command_list(args: argparse.Namespace) -> None:
    _config, queue, _archive = require_initialized(args.root)
    if args.json:
        print(json.dumps(queue, ensure_ascii=False, indent=2))
        return
    for section in ("active", "pending", "blocked", "completed"):
        print(f"{section} ({len(queue.get(section, []))})")
        for entry in queue.get(section, []):
            mode = entry.get("workflow_mode", "lite")
            print(f"  {entry['id']}  [{mode}]  {entry.get('status', section)}  {entry.get('name', '')}")


def parser() -> argparse.ArgumentParser:
    top = argparse.ArgumentParser(description=__doc__)
    top.add_argument("--root", type=Path, default=Path.cwd(), help="project root (default: current directory)")
    commands = top.add_subparsers(dest="command", required=True)

    init = commands.add_parser("init")
    init.add_argument("--project-name")
    init.add_argument("--main-branch", default="main")
    init.add_argument("--max-concurrent", type=int, default=2)
    init.set_defaults(handler=command_init)

    create = commands.add_parser("create")
    create.add_argument("--id", required=True)
    create.add_argument("--name", required=True)
    create.add_argument("--description", required=True)
    create.add_argument("--priority", type=int, default=50)
    create.add_argument("--mode", choices=("auto", "lite", "deep"), default="auto")
    create.add_argument("--signals", default="")
    create.add_argument("--dependencies", default="")
    create.set_defaults(handler=command_create)

    listing = commands.add_parser("list")
    listing.add_argument("--json", action="store_true")
    listing.set_defaults(handler=command_list)

    start = commands.add_parser("start-state")
    start.add_argument("--id", required=True)
    start.add_argument("--branch", required=True)
    start.add_argument("--worktree", required=True)
    start.set_defaults(handler=command_start_state)

    attention = commands.add_parser("needs-attention")
    attention.add_argument("--id", required=True)
    attention.add_argument("--reason", required=True)
    attention.set_defaults(handler=command_needs_attention)

    block = commands.add_parser("block")
    block.add_argument("--id", required=True)
    block.add_argument("--reason", required=True)
    block.set_defaults(handler=lambda value: command_move(value, "blocked"))

    unblock = commands.add_parser("unblock")
    unblock.add_argument("--id", required=True)
    unblock.set_defaults(handler=lambda value: command_move(value, "pending"))

    validate = commands.add_parser("validate-completion")
    validate.add_argument("--id", required=True)
    validate.add_argument("--skip-checklist", action="store_true")
    validate.set_defaults(handler=command_validate_completion)

    complete = commands.add_parser("complete-state")
    complete.add_argument("--id", required=True)
    complete.add_argument("--archive-path", required=True)
    complete.add_argument("--tag", required=True)
    complete.add_argument("--merge-commit", required=True)
    complete.add_argument("--skip-checklist", action="store_true")
    complete.set_defaults(handler=command_complete_state)
    return top


def main() -> int:
    args = parser().parse_args()
    args.root = args.root.resolve()
    try:
        args.handler(args)
    except WorkflowError as exc:
        print(json.dumps({"status": "error", "error": str(exc)}, ensure_ascii=False), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
