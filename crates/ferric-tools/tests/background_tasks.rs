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
    let ctx = ToolCtx { workspace: &workspace };

    let shell_exec = ShellExec;
    let manage_task = ManageTask;

    // 1. Spawn a background task
    let command = if cfg!(windows) {
        "timeout /t 5"
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
