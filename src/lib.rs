#[cfg(feature = "gui")]
pub mod app;
pub use quartz_project as core;
pub mod mcp;
pub mod mcp_data;
pub mod mcp_effects;
pub mod mcp_shaders;
pub mod mcp_gaps;
/// Editor plugins: each folder under src/plugin/ (see build.rs).
#[cfg(feature = "gui")]
pub mod plugin;
pub mod services;
