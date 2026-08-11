//! Codex agent conversion provided by `cmx-core`.

pub use cmx_core::agent::markdown_to_codex_toml;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_full_agent_to_toml() {
        let md = "---\nname: rust-craftsperson\ndescription: An expert Rust agent\nmodel: gpt-5\n---\nYou are a meticulous Rust engineer.\n";
        let toml = markdown_to_codex_toml(md, "fallback");
        assert!(toml.contains("name = \"rust-craftsperson\"\n"));
        assert!(toml.contains("description = \"An expert Rust agent\"\n"));
        assert!(toml.contains("model = \"gpt-5\"\n"));
        assert!(
            toml.contains("developer_instructions = \"You are a meticulous Rust engineer.\"\n")
        );
    }

    #[test]
    fn uses_fallback_name_without_frontmatter() {
        let toml = markdown_to_codex_toml("Instructions.\n", "helper");
        assert!(toml.contains("name = \"helper\"\n"));
        assert!(toml.contains("developer_instructions = \"Instructions.\"\n"));
    }
}
