//! The studio's agent registry and skills configuration, loaded fail-closed
//! at `scarced` boot (GUIDELINES.md §3–§4).
//!
//! Three in-repo artifacts, one loader:
//! - `agents/<name>.persona.md` — Buzz's persona format, verbatim (parsed by
//!   the `buzz-persona` crate; strict frontmatter, typos are parse errors)
//! - `roster.toml` — studio economics keyed by persona `name` (npub binding,
//!   skill tags, day rate)
//! - `skills.toml` — pinned skill sources the persona `skills:` slugs
//!   resolve through
//!
//! A registry that does not parse is a `scarced` that does not start: every
//! error names the file and the fix. The registry *configures* agents; it
//! does not mint them — provisioning an npub stays an owner ceremony.

mod roster;
mod skills;

pub use roster::RosterEntry;
pub use skills::{SkillDef, Skills};

use std::collections::BTreeMap;
use std::path::Path;

use buzz_persona::persona::{parse_persona_file, PersonaConfig};

/// One staffable agent: runtime settings from the persona file, studio
/// economics from the roster, joined on `name`.
#[derive(Debug, Clone)]
pub struct Agent {
    pub persona: PersonaConfig,
    pub roster: RosterEntry,
}

/// The loaded registry the orchestrator staffs workrooms from.
#[derive(Debug, Clone)]
pub struct Registry {
    /// Agents keyed by persona `name`.
    pub agents: BTreeMap<String, Agent>,
    pub skills: Skills,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("agents directory not found at {path} — the registry is one `agents/<name>.persona.md` per agent (GUIDELINES.md §3); create it or point `registry_dir` at the studio repo")]
    MissingAgentsDir { path: String },

    #[error(
        "no persona files in {path} — the registry needs at least one `agents/<name>.persona.md`"
    )]
    EmptyRegistry { path: String },

    #[error("unexpected file {path} in agents/ — only `<name>.persona.md` files (and README.md) live here; rename or remove it")]
    UnexpectedFile { path: String },

    #[error("persona {path}: {message}")]
    Persona { path: String, message: String },

    #[error("persona {path}: frontmatter name is `{name}` but the file is not named `{name}.persona.md` — rename one so the registry key is unambiguous")]
    NameMismatch { path: String, name: String },

    #[error("{path}: {message}")]
    File { path: String, message: String },

    #[error("roster entry `{name}`: {message}")]
    Roster { name: String, message: String },

    #[error("roster entry `{name}` has no persona file — every roster name must match an `agents/{name}.persona.md`")]
    RosterWithoutPersona { name: String },

    #[error("persona `{name}` has no roster entry — add `[agents.{name}]` with its npub to roster.toml (provision the identity first if it has none)")]
    PersonaWithoutRoster { name: String },

    #[error("skill `{slug}` (referenced by {referenced_by}) is not defined in skills.toml — add a `[skills.{slug}]` entry with a pinned rev")]
    UnknownSkill { slug: String, referenced_by: String },

    #[error("skill `{slug}`: {message}")]
    Skill { slug: String, message: String },
}

/// Load the registry from a studio repo root: `<root>/agents/*.persona.md`,
/// `<root>/roster.toml`, `<root>/skills.toml`. Fail-closed — the first
/// structural problem aborts the load with an actionable error.
pub fn load(root: &Path) -> Result<Registry, RegistryError> {
    let skills = skills::load(&root.join("skills.toml"))?;
    let mut roster = roster::load(&root.join("roster.toml"))?;

    let agents_dir = root.join("agents");
    if !agents_dir.is_dir() {
        return Err(RegistryError::MissingAgentsDir {
            path: agents_dir.display().to_string(),
        });
    }

    let mut entries: Vec<_> = std::fs::read_dir(&agents_dir)
        .map_err(|e| RegistryError::File {
            path: agents_dir.display().to_string(),
            message: e.to_string(),
        })?
        .collect::<Result<_, _>>()
        .map_err(|e| RegistryError::File {
            path: agents_dir.display().to_string(),
            message: e.to_string(),
        })?;
    entries.sort_by_key(|e| e.file_name());

    let mut agents = BTreeMap::new();
    for entry in entries {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        // Dotfiles and the directory's own README are not registry entries.
        if file_name.starts_with('.') || file_name == "README.md" {
            continue;
        }
        let Some(stem) = file_name.strip_suffix(".persona.md") else {
            // Fail closed: a typo'd extension must not silently drop an agent.
            return Err(RegistryError::UnexpectedFile {
                path: path.display().to_string(),
            });
        };

        let persona = parse_persona_file(&path).map_err(|e| RegistryError::Persona {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        if persona.name != stem {
            return Err(RegistryError::NameMismatch {
                path: path.display().to_string(),
                name: persona.name,
            });
        }

        for slug in &persona.skills {
            if !skills.skills.contains_key(slug) {
                return Err(RegistryError::UnknownSkill {
                    slug: slug.clone(),
                    referenced_by: format!("persona `{}`", persona.name),
                });
            }
        }

        let roster_entry =
            roster
                .remove(&persona.name)
                .ok_or_else(|| RegistryError::PersonaWithoutRoster {
                    name: persona.name.clone(),
                })?;

        agents.insert(
            persona.name.clone(),
            Agent {
                persona,
                roster: roster_entry,
            },
        );
    }

    // Whatever is left in the roster references a persona that does not exist.
    if let Some(name) = roster.into_keys().next() {
        return Err(RegistryError::RosterWithoutPersona { name });
    }
    if agents.is_empty() {
        return Err(RegistryError::EmptyRegistry {
            path: agents_dir.display().to_string(),
        });
    }

    Ok(Registry { agents, skills })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NPUB: &str = "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy";
    const REV: &str = "3875492f6f70fc694bff64167fe0a0d14f346503";

    fn persona(name: &str, skills: &str) -> String {
        format!(
            "---\nname: {name}\ndisplay_name: {name}\ndescription: test agent\nmodel: anthropic:claude-fable-5\nruntime: claude\nskills:{skills}\n---\n\nYou are {name}.\n"
        )
    }

    /// A studio root with one agent wired end-to-end.
    fn studio(dir: &tempfile::TempDir) {
        let root = dir.path();
        std::fs::create_dir(root.join("agents")).unwrap();
        std::fs::write(
            root.join("agents/ruben.persona.md"),
            persona("ruben", "\n  - sf-rust"),
        )
        .unwrap();
        std::fs::write(
            root.join("roster.toml"),
            format!("[agents.ruben]\nnpub = \"{NPUB}\"\nskill_tags = [\"rust\"]\n"),
        )
        .unwrap();
        std::fs::write(
            root.join("skills.toml"),
            format!("[skills.sf-rust]\nsource = \"github:solana-foundation/ai-skills\"\npath = \"sf-rust-skill\"\nrev = \"{REV}\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn full_registry_loads() {
        let dir = tempfile::tempdir().unwrap();
        studio(&dir);
        let registry = load(dir.path()).expect("loads");
        let ruben = &registry.agents["ruben"];
        assert_eq!(
            ruben.persona.model.as_deref(),
            Some("anthropic:claude-fable-5")
        );
        assert_eq!(ruben.roster.npub, NPUB);
        assert!(ruben.persona.prompt.contains("You are ruben"));
    }

    #[test]
    fn frontmatter_typo_fails_boot_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        studio(&dir);
        std::fs::write(
            dir.path().join("agents/ruben.persona.md"),
            "---\nname: ruben\ndisplay_name: ruben\ndescription: d\nmodell: oops\n---\nbody\n",
        )
        .unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("ruben.persona.md") && err.contains("modell"),
            "{err}"
        );
    }

    #[test]
    fn filename_must_match_persona_name() {
        let dir = tempfile::tempdir().unwrap();
        studio(&dir);
        std::fs::write(
            dir.path().join("agents/rube.persona.md"),
            persona("ruben", " []"),
        )
        .unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("rube.persona.md") && err.contains("`ruben`"),
            "{err}"
        );
    }

    #[test]
    fn unexpected_file_fails_readme_and_dotfiles_pass() {
        let dir = tempfile::tempdir().unwrap();
        studio(&dir);
        std::fs::write(dir.path().join("agents/README.md"), "roster docs").unwrap();
        std::fs::write(dir.path().join("agents/.DS_Store"), "").unwrap();
        load(dir.path()).expect("README + dotfiles ignored");

        std::fs::write(dir.path().join("agents/ruben.persona"), "typo").unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("ruben.persona"), "{err}");
    }

    #[test]
    fn persona_skill_slug_must_resolve() {
        let dir = tempfile::tempdir().unwrap();
        studio(&dir);
        std::fs::write(
            dir.path().join("agents/ruben.persona.md"),
            persona("ruben", "\n  - sf-ghost"),
        )
        .unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("sf-ghost") && err.contains("persona `ruben`"),
            "{err}"
        );
    }

    #[test]
    fn roster_and_personas_must_be_in_bijection() {
        let dir = tempfile::tempdir().unwrap();
        studio(&dir);
        std::fs::write(
            dir.path().join("roster.toml"),
            format!("[agents.ghost]\nnpub = \"{NPUB}\"\n"),
        )
        .unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        // ruben has no roster entry — that error fires first, deterministically.
        assert!(err.contains("persona `ruben` has no roster entry"), "{err}");

        std::fs::write(
            dir.path().join("roster.toml"),
            format!("[agents.ruben]\nnpub = \"{NPUB}\"\n[agents.ghost]\nnpub = \"{NPUB}\"\n"),
        )
        .unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("`ghost` has no persona file"), "{err}");
    }

    #[test]
    fn empty_agents_dir_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        studio(&dir);
        std::fs::remove_file(dir.path().join("agents/ruben.persona.md")).unwrap();
        std::fs::write(dir.path().join("roster.toml"), "").unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("no persona files"), "{err}");
    }

    #[test]
    fn missing_agents_dir_error_is_actionable() {
        let dir = tempfile::tempdir().unwrap();
        studio(&dir);
        std::fs::remove_dir_all(dir.path().join("agents")).unwrap();
        let err = load(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("agents") && err.contains("registry_dir"),
            "{err}"
        );
    }
}
