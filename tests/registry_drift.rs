//! The repo's own registry files must load — the same drift-guard idea as
//! the generated-schema test: scarced boots fail-closed on these, so a
//! commit that breaks `agents/`, `roster.toml` or `skills.toml` fails here
//! instead of at the next deploy.

use std::path::Path;

#[test]
fn repo_registry_loads() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let registry = studio_registry::load(root).expect("repo registry must parse");

    let ruben = registry
        .agents
        .get("ruben")
        .expect("ruben is the first registry entry");
    assert_eq!(ruben.persona.runtime.as_deref(), Some("claude"));
    assert!(!ruben.persona.prompt.trim().is_empty());
    assert!(ruben.roster.npub.starts_with("npub1"));

    // Every default role's slugs resolved at load; spot-check the backend one
    // that GUIDELINES.md §1 makes mandatory.
    assert!(registry.skills.defaults["backend"].contains(&"sf-rust".to_string()));
}
