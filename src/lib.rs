#[cfg(feature = "gui")]
pub mod app;
pub mod core;
pub mod mcp;
/// Editor plugins: each folder under src/plugin/ (see build.rs).
#[cfg(feature = "gui")]
pub mod plugin;
pub mod services;
