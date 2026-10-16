//! Editor plugins, as quartz has them: every folder here with a mod.rs is compiled in by build.rs
//! (with `#[cfg(plugin_<name>)]` true for code elsewhere that uses it). Delete a folder and the
//! editor builds without that plugin.
//!
//! - `path_forge`: previews of PathForge backgrounds, from PathForge's runtime library loaded at
//!   run time (PathForge is not needed to build the editor).

include!(concat!(env!("OUT_DIR"), "/plugin_mods.rs"));
