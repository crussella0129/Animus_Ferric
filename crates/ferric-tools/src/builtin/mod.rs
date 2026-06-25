//! Builtin tools. All are NANO-tier: the simple operations small models must
//! get 100% right are exactly the ones every tier needs available.

mod copy_file;
mod delete_path;
mod edit_file;
mod find_files;
mod list_dir;
mod make_dir;
mod move_path;
mod read_file;
mod search_files;
mod write_file;

pub use copy_file::CopyFile;
pub use delete_path::DeletePath;
pub use edit_file::EditFile;
pub use find_files::FindFiles;
pub use list_dir::ListDir;
pub use make_dir::MakeDir;
pub use move_path::MovePath;
pub use read_file::ReadFile;
pub use search_files::SearchFiles;
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
}

/// Shared helper: the required string `path` argument.
fn path_arg(args: &serde_json::Value) -> Result<&str, String> {
    args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing required string argument: path".to_string())
}
