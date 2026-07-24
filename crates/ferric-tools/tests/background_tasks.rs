use ferric_guard::Workspace;
use ferric_tools::builtin::{ManageTask, ShellExec};
use ferric_tools::{Tool, ToolCtx};
use serde_json::json;
use std::time::Duration;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread")]
async fn test_background_tasks() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let workspace = Workspace::new(&root).unwrap();
    let ctx = ToolCtx {
        workspace: &workspace,
    };

    let shell_exec = ShellExec;
    let manage_task = ManageTask;

    // 1. Spawn a background task
    let command = if cfg!(windows) {
        "powershell -Command \"Start-Sleep -Seconds 5\""
    } else {
        "sleep 5"
    };

    let shell_args = json!({
        "command": command,
        "background": true,
    });

    let res = shell_exec.run(&ctx, &shell_args).unwrap();
    assert!(res.contains("Started background task"));

    // Extract task id (e.g. "task-1234")
    let start_idx = res.find("task-").unwrap();
    let end_idx = res[start_idx..].find(".").unwrap();
    let task_id = &res[start_idx..start_idx + end_idx];

    // 2. List tasks (should be Running)
    let list_args = json!({ "action": "list" });
    let list_res = manage_task.run(&ctx, &list_args).unwrap();
    assert!(list_res.contains(task_id));
    assert!(list_res.contains("Running"));

    // 3. Status
    let status_args = json!({
        "action": "status",
        "task_id": task_id,
    });
    let status_res = manage_task.run(&ctx, &status_args).unwrap();
    assert!(status_res.contains("Status: Running"));
    assert!(status_res.contains(command));

    // 4. Kill the task
    let kill_args = json!({
        "action": "kill",
        "task_id": task_id,
    });
    let kill_res = manage_task.run(&ctx, &kill_args).unwrap();
    assert!(kill_res.contains("killed successfully"));

    // 5. Verify it is Terminated
    // Let's give the OS a split second to reap the child
    tokio::time::sleep(Duration::from_millis(100)).await;

    let list_res2 = manage_task.run(&ctx, &list_args).unwrap();
    assert!(list_res2.contains("Terminated"));
}

// --- ADR-074: the paths that used to abort the harness ---

/// `send_input` called `Handle::current()` (panics with no runtime) and
/// `block_in_place` (panics on a current-thread runtime), while `ferric-loop`
/// is explicitly executor-agnostic. Both must now be ordinary tool errors.
///
/// The task is spawned on a multi-thread runtime (background spawn needs one),
/// then `send_input` is driven from a *separate thread* carrying a
/// current-thread runtime — the exact shape that used to abort the process.
#[tokio::test(flavor = "multi_thread")]
async fn send_input_from_a_current_thread_runtime_errors_instead_of_panicking() {
    let dir = tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let ctx = ToolCtx {
        workspace: &workspace,
    };

    let command = if cfg!(windows) {
        "powershell -Command \"Start-Sleep -Seconds 5\""
    } else {
        "sleep 5"
    };
    let res = ShellExec
        .run(&ctx, &json!({ "command": command, "background": true }))
        .unwrap();
    let start = res.find("task-").unwrap();
    let end = res[start..].find('.').unwrap();
    let task_id = res[start..start + end].to_string();

    let root = dir.path().to_path_buf();
    let id = task_id.clone();
    let outcome = std::thread::spawn(move || {
        let ws = Workspace::new(&root).unwrap();
        let ctx = ToolCtx { workspace: &ws };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            ManageTask.run(
                &ctx,
                &json!({ "action": "send_input", "task_id": id, "input": "hi\n" }),
            )
        })
    })
    .join()
    .expect("the tool must not panic the thread");

    let err = outcome.expect_err("expected a reported error, not success");
    assert!(
        err.contains("multi-thread"),
        "the error should say why: {err}"
    );

    let _ = ManageTask.run(&ctx, &json!({ "action": "kill", "task_id": &task_id }));
    let _ = ManageTask.run(&ctx, &json!({ "action": "remove", "task_id": &task_id }));
}

/// The companion case: with a proper multi-thread runtime, `send_input` works.
#[tokio::test(flavor = "multi_thread")]
async fn send_input_succeeds_on_a_multi_thread_runtime() {
    let dir = tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let ctx = ToolCtx {
        workspace: &workspace,
    };

    let command = if cfg!(windows) {
        "powershell -Command \"Start-Sleep -Seconds 5\""
    } else {
        "sleep 5"
    };
    let res = ShellExec
        .run(&ctx, &json!({ "command": command, "background": true }))
        .unwrap();
    let start = res.find("task-").unwrap();
    let end = res[start..].find('.').unwrap();
    let task_id = res[start..start + end].to_string();

    let out = ManageTask
        .run(
            &ctx,
            &json!({ "action": "send_input", "task_id": &task_id, "input": "hi\n" }),
        )
        .expect("send_input should work on a multi-thread runtime");
    assert!(out.contains("Sent input"), "got: {out}");

    // And the pipe survives, so a second write also lands — the old take/restore
    // dance could lose it permanently.
    ManageTask
        .run(
            &ctx,
            &json!({ "action": "send_input", "task_id": &task_id, "input": "again\n" }),
        )
        .expect("stdin must still be there for a second write");

    let _ = ManageTask.run(&ctx, &json!({ "action": "kill", "task_id": &task_id }));
    let _ = ManageTask.run(&ctx, &json!({ "action": "remove", "task_id": &task_id }));
}

/// A missing task is a normal error on every action — never a panic, and never
/// a silent success.
#[tokio::test(flavor = "multi_thread")]
async fn unknown_task_is_an_error_on_every_action() {
    let dir = tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let ctx = ToolCtx {
        workspace: &workspace,
    };

    for action in ["status", "kill"] {
        let out = ManageTask.run(&ctx, &json!({ "action": action, "task_id": "task-nope" }));
        assert!(out.is_err(), "{action} on a missing task should error");
    }
    let out = ManageTask
        .run(
            &ctx,
            &json!({ "action": "send_input", "task_id": "task-nope", "input": "x" }),
        )
        .unwrap_err();
    assert!(out.contains("not found"), "got: {out}");
}

/// C4: the registry had no removal path at all, so tasks and their `Child`
/// handles accumulated for the life of the process.
#[tokio::test(flavor = "multi_thread")]
async fn finished_tasks_can_be_removed() {
    let dir = tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let ctx = ToolCtx {
        workspace: &workspace,
    };

    let command = if cfg!(windows) {
        "powershell -Command \"Start-Sleep -Seconds 5\""
    } else {
        "sleep 5"
    };
    let res = ShellExec
        .run(&ctx, &json!({ "command": command, "background": true }))
        .unwrap();
    let start = res.find("task-").unwrap();
    let end = res[start..].find('.').unwrap();
    let task_id = &res[start..start + end].to_string();

    ManageTask
        .run(&ctx, &json!({ "action": "kill", "task_id": task_id }))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Still listed after it stops — the status IS the record.
    let listed = ManageTask.run(&ctx, &json!({ "action": "list" })).unwrap();
    assert!(listed.contains(task_id.as_str()));

    let removed = ManageTask
        .run(&ctx, &json!({ "action": "remove", "task_id": task_id }))
        .unwrap();
    assert!(removed.contains("Removed task"), "got: {removed}");

    let after = ManageTask.run(&ctx, &json!({ "action": "list" })).unwrap();
    assert!(
        !after.contains(task_id.as_str()),
        "removed task must be gone, got: {after}"
    );
}

/// Regression (ADR-074): background-task ids were `task-{millis}`, so two tasks
/// started inside the same millisecond collided. The registry is keyed by id, so
/// the second silently evicted the first — its `Child` handle gone, the task
/// unlistable, uninspectable, unkillable. Found because it made two tests in
/// this file flake against each other.
#[tokio::test(flavor = "multi_thread")]
async fn back_to_back_tasks_get_distinct_ids() {
    let dir = tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let ctx = ToolCtx {
        workspace: &workspace,
    };

    let command = if cfg!(windows) {
        "powershell -Command \"Start-Sleep -Seconds 5\""
    } else {
        "sleep 5"
    };

    let mut ids = Vec::new();
    for _ in 0..5 {
        let res = ShellExec
            .run(&ctx, &json!({ "command": command, "background": true }))
            .unwrap();
        let start = res.find("task-").unwrap();
        let end = res[start..].find('.').unwrap();
        ids.push(res[start..start + end].to_string());
    }

    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        ids.len(),
        "ids must be unique even started back-to-back, got: {ids:?}"
    );

    // Every one of them is really in the registry — the point of uniqueness.
    let listed = ManageTask.run(&ctx, &json!({ "action": "list" })).unwrap();
    for id in &ids {
        assert!(listed.contains(id.as_str()), "{id} missing from: {listed}");
        let _ = ManageTask.run(&ctx, &json!({ "action": "kill", "task_id": id }));
        let _ = ManageTask.run(&ctx, &json!({ "action": "remove", "task_id": id }));
    }
}
