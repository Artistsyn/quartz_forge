//! The quartz_forge MCP server without the editor: the same `quartz_forge::mcp`
//! as the editor crate's `quartz_forge_mcp` binary, built from the library with
//! the `gui` feature off (no egui, eframe, image or PathForge).
fn main() -> anyhow::Result<()> {
    quartz_forge::mcp::run_from_args()
}
