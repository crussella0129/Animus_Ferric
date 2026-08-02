//! Builtin tools offered to models, plus explicitly human-only host controls.

mod apply_patch;
mod blocking;
pub(crate) mod check_syntax;
pub(crate) mod controlled_file;
pub(crate) mod controlled_read;
mod copy_file;
mod delete_path;
mod edit_file;
mod fetch_reference;
mod find_files;
mod git_read;
mod git_write;
mod list_dir;
mod make_dir;
mod manage_task;
mod move_path;
mod multi_edit;
mod read_file;
mod run_check;
mod search_files;
mod shell_exec;
pub mod task_registry;
mod write_file;

pub use apply_patch::ApplyPatch;
pub use copy_file::CopyFile;
pub use delete_path::DeletePath;
pub use edit_file::EditFile;
pub use fetch_reference::FetchReference;
pub use find_files::FindFiles;
pub use git_read::GitRead;
pub use git_write::GitWrite;
pub use list_dir::ListDir;
pub use make_dir::MakeDir;
pub use manage_task::ManageTask;
pub use move_path::MovePath;
pub use multi_edit::MultiEdit;
pub use read_file::ReadFile;
pub use run_check::{NamedCheck, RunCheck};
pub use search_files::SearchFiles;
pub use shell_exec::ShellExec;
pub use write_file::WriteFile;

use crate::registry::Registry;

/// Register the builtin tool set.
pub fn register_builtin_tools(registry: &mut Registry) {
    registry.register(Box::new(ReadFile));
    registry.register(Box::new(WriteFile));
    registry.register(Box::new(EditFile));
    registry.register(Box::new(ListDir));
    registry.register(Box::new(MovePath));
    registry.register(Box::new(MakeDir));
    registry.register(Box::new(SearchFiles));
    registry.register(Box::new(DeletePath));
    registry.register(Box::new(FindFiles));
    registry.register(Box::new(CopyFile));
    registry.register(Box::new(MultiEdit));
    registry.register(Box::new(ApplyPatch));
    registry.register(Box::new(GitRead));
    registry.register(Box::new(GitWrite));
}

/// Register the operator-authorized verification tool and retain the complete
/// required-check set for the loop's completion-evidence gate.
pub fn register_run_checks(registry: &mut Registry, checks: Vec<NamedCheck>) -> Result<(), String> {
    let tool = RunCheck::new(checks)?;
    registry.set_required_checks(tool.names());
    registry.register(Box::new(tool));
    Ok(())
}

/// Register host-shell controls for an explicit human surface.
///
/// These tools use the host OS with the workspace as their working directory;
/// that is not filesystem containment. They must never share the registry used
/// to build a model's grammar or dispatch model-authored calls.
pub fn register_human_tools(registry: &mut Registry) {
    registry.register(Box::new(ShellExec));
    registry.register(Box::new(ManageTask));
}

/// Shared helper: the required string `path` argument.
fn path_arg(args: &serde_json::Value) -> Result<&str, String> {
    args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing required string argument: path".to_string())
}
