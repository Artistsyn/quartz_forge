pub mod core {
    #[path = "../../../src/core/layout.rs"]
    pub mod layout;
    #[path = "../../../src/core/project.rs"]
    pub mod project;
    #[path = "../../../src/core/quartz_domain.rs"]
    pub mod quartz_domain;
}

pub mod services {
    #[path = "../../../src/services/codegen_text.rs"]
    pub mod codegen_text;
    #[path = "../../../src/services/codegen.rs"]
    pub mod codegen;
    #[path = "../../../src/services/project_import.rs"]
    pub mod project_import;
    #[path = "../../../src/services/persistence.rs"]
    pub mod persistence;
    #[path = "../../../src/services/project_sync.rs"]
    pub mod project_sync;
}

#[path = "../../src/mcp.rs"]
pub mod mcp;

fn main() -> anyhow::Result<()> {
    mcp::run_from_args()
}