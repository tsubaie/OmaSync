//! Everything that touches chezmoi, git, gh and pacman. No UI code here. One module per job;
//! `main.rs` reaches only what's re-exported below.

mod apps;
mod background;
mod bar;
mod choices;
mod ctx;
mod data;
mod engine;
mod history;
#[cfg(test)]
mod lab;
mod overview;
mod plugins;
mod rules;
#[cfg(test)]
mod scenarios;
mod setup;
mod stage;

pub use apps::{
    add_repository, install_apps, install_with_script, installed, is_package_name, missing_apps,
    missing_repos, missing_scripted, repos_here,
};
pub use background::{background, schedule};
pub use bar::{bar_indicator, ensure_bar_indicator, set_bar_indicator};
pub use ctx::{Ctx, edit, friendly, is_git_address, run, sync_space};
// What the updater shares with a sync.
pub(crate) use ctx::{lock, lock_wait, plan};
pub use data::{Data, Script, is_plugin_id, name_taken, toggle};
pub use engine::{Conflict, Keep, Report, SyncError, disconnect, put_back_omarchy, resolve, sync};
pub use history::undo;
pub use overview::{Overview, overview};
pub use plugins::{add_plugin, install_plugins, remove_plugin, set_plugin_enabled, update_plugins};
pub use rules::{DATA_LABEL, FOLDERS};
pub use setup::{SetupState, install_helpers, join, probe, sign_in, start};
pub use stage::{track, track_plugin};

// Shared by the modules, each of which starts with `use super::*`.
use apps::*;
use background::*;
use bar::*;
use ctx::*;
use data::*;
use engine::*;
use history::*;
use overview::*;
use plugins::*;
use rules::*;
use serde::{Deserialize, Serialize};
use setup::*;
use stage::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
