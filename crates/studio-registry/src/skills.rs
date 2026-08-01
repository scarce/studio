//! `skills.toml` — which conventions govern the builds (GUIDELINES.md §4).
//! Personas reference skills by slug; this file says from where, at what
//! version, with what config. Revs are pinned commits, never branches: a
//! skill is a dependency of the build's correctness.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::RegistryError;

/// Parsed, validated `skills.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skills {
    /// Skill definitions keyed by slug — the slugs persona `skills:` lists
    /// resolve against.
    #[serde(default)]
    pub skills: BTreeMap<String, SkillDef>,
    /// Role → slugs applied to every crew member of that role
    /// (e.g. `backend = ["sf-rust"]`).
    #[serde(default)]
    pub defaults: BTreeMap<String, Vec<String>>,
    /// Per-skill studio config, passed to the skill's context when loaded.
    #[serde(default)]
    pub overrides: BTreeMap<String, toml::Table>,
}

/// One pinned skill source.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDef {
    /// Where the skill lives, e.g. `github:solana-foundation/ai-skills`.
    pub source: String,
    /// Path inside the source, e.g. `sf-rust-skill`. Omitted = source root.
    #[serde(default)]
    pub path: Option<String>,
    /// Pinned commit — full 40-hex, never a branch or tag.
    pub rev: String,
}

fn is_full_commit(rev: &str) -> bool {
    rev.len() == 40
        && rev
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Load and validate `skills.toml`. Fail-closed: any structural problem is
/// an error naming the file, the slug, and what to fix.
pub fn load(path: &Path) -> Result<Skills, RegistryError> {
    let content = std::fs::read_to_string(path).map_err(|e| RegistryError::File {
        path: path.display().to_string(),
        message: format!("{e} — the studio's skill pins live here (GUIDELINES.md §4)"),
    })?;
    let skills: Skills = toml::from_str(&content).map_err(|e| RegistryError::File {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    for (slug, def) in &skills.skills {
        if def.source.trim().is_empty() {
            return Err(RegistryError::Skill {
                slug: slug.clone(),
                message: "source must name where the skill lives, e.g. `github:solana-foundation/ai-skills`".into(),
            });
        }
        if !is_full_commit(&def.rev) {
            return Err(RegistryError::Skill {
                slug: slug.clone(),
                message: format!(
                    "rev `{}` is not a pinned commit — use the full 40-hex commit hash, never a branch or tag",
                    def.rev
                ),
            });
        }
    }
    for (role, slugs) in &skills.defaults {
        for slug in slugs {
            if !skills.skills.contains_key(slug) {
                return Err(RegistryError::UnknownSkill {
                    slug: slug.clone(),
                    referenced_by: format!("defaults.{role} in skills.toml"),
                });
            }
        }
    }
    for slug in skills.overrides.keys() {
        if !skills.skills.contains_key(slug) {
            return Err(RegistryError::UnknownSkill {
                slug: slug.clone(),
                referenced_by: "overrides in skills.toml".into(),
            });
        }
    }
    Ok(skills)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &tempfile::TempDir, content: &str) -> std::path::PathBuf {
        let path = dir.path().join("skills.toml");
        std::fs::write(&path, content).unwrap();
        path
    }

    const GOOD: &str = r#"
[skills.sf-rust]
source = "github:solana-foundation/ai-skills"
path   = "sf-rust-skill"
rev    = "3875492f6f70fc694bff64167fe0a0d14f346503"

[defaults]
backend = ["sf-rust"]

[overrides]
sf-rust = { edition = "2021" }
"#;

    #[test]
    fn good_file_parses() {
        let dir = tempfile::tempdir().unwrap();
        let skills = load(&write(&dir, GOOD)).expect("parses");
        assert_eq!(
            skills.skills["sf-rust"].path.as_deref(),
            Some("sf-rust-skill")
        );
        assert_eq!(skills.defaults["backend"], vec!["sf-rust"]);
    }

    #[test]
    fn branch_rev_rejected_with_pin_instruction() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            "[skills.sf-rust]\nsource = \"github:solana-foundation/ai-skills\"\nrev = \"main\"\n",
        );
        let err = load(&path).unwrap_err().to_string();
        assert!(err.contains("main") && err.contains("40-hex"), "{err}");
    }

    #[test]
    fn short_rev_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            "[skills.sf-rust]\nsource = \"g\"\nrev = \"3875492\"\n",
        );
        let err = load(&path).unwrap_err().to_string();
        assert!(err.contains("40-hex"), "{err}");
    }

    #[test]
    fn unknown_key_rejected_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "[skillz]\n");
        let err = load(&path).unwrap_err().to_string();
        assert!(err.contains("skillz"), "{err}");
    }

    #[test]
    fn default_and_override_slugs_must_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "[defaults]\nbackend = [\"sf-rust\"]\n");
        let err = load(&path).unwrap_err().to_string();
        assert!(
            err.contains("sf-rust") && err.contains("defaults.backend"),
            "{err}"
        );

        let path = write(&dir, "[overrides]\nghost = { a = 1 }\n");
        let err = load(&path).unwrap_err().to_string();
        assert!(err.contains("ghost"), "{err}");
    }

    #[test]
    fn missing_file_error_names_path() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(&dir.path().join("skills.toml"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("skills.toml"), "{err}");
    }
}
