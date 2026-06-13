//! Builtin tools. All are NANO-tier: the simple operations small models must
//! get 100% right are exactly the ones every tier needs available.

mod list_dir;
mod make_dir;
mod move_path;
mod read_file;
mod write_file;

pub use list_dir::ListDir;
pub use make_dir::MakeDir;
pub use move_path::MovePath;
pub use read_file::ReadFile;
pub use write_file::WriteFile;

use crate::registry::Registry;

/// Register the builtin tool set.
pub fn register_builtin_tools(registry: &mut Registry) {
    registry.register(Box::new(ReadFile));
    registry.register(Box::new(WriteFile));
    registry.register(Box::new(ListDir));
    registry.register(Box::new(MovePath));
    registry.register(Box::new(MakeDir));
}

/// Shared helper: the required string `path` argument.
fn path_arg(args: &serde_json::Value) -> Result<&str, String> {
    args.get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing required string argument: path".to_string())
}
