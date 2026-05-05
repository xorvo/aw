//! Workspace lifecycle: ports of the bash CLI's `init`, `create`, `start`,
//! `list`, `delete`, `config`, `edit-*`, `sync`, `open-home` subcommands.

pub mod config_show;
pub mod create;
pub mod delete;
pub mod edit;
pub mod init;
pub mod list;
pub mod listing;
pub mod meta;
pub mod start;
pub mod sync;
