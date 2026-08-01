//! `roster.toml` — studio economics, keyed by persona `name`
//! (GUIDELINES.md §3.3). The persona file carries what the runtime needs;
//! this file carries what the studio needs: identity binding and money.
//! Strict frontmatter will not accept foreign keys, so these never live in
//! the `.persona.md`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::RegistryError;

/// Raw `roster.toml`: `[agents.<name>]` tables.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RosterFile {
    #[serde(default)]
    agents: BTreeMap<String, RosterEntry>,
}

/// Studio-side record for one agent — joined to the persona by `name`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RosterEntry {
    /// Nostr identity the agent signs with (structural check at load).
    pub npub: String,
    /// Payout key on Solana (ARCHITECTURE.md §5). Optional until the
    /// key-binding ceremony has run.
    #[serde(default)]
    pub solana_pubkey: Option<String>,
    /// Reference to the npub↔Solana key-binding attestation event.
    #[serde(default)]
    pub attestation: Option<String>,
    /// Tags crew selection matches quotes against, e.g. `["rust", "buzz"]`.
    #[serde(default)]
    pub skill_tags: Vec<String>,
    /// Day rate in token minor units. Unset = not yet priced by the owner.
    #[serde(default)]
    pub day_rate: Option<studio_types::Amount>,
}

/// Load `roster.toml` and validate every npub structurally.
pub fn load(path: &Path) -> Result<BTreeMap<String, RosterEntry>, RegistryError> {
    let content = std::fs::read_to_string(path).map_err(|e| RegistryError::File {
        path: path.display().to_string(),
        message: format!(
            "{e} — the roster binds persona names to npubs and rates (GUIDELINES.md §3.3)"
        ),
    })?;
    let roster: RosterFile = toml::from_str(&content).map_err(|e| RegistryError::File {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    for (name, entry) in &roster.agents {
        if let Err(message) = studio_types::validate_npub(&entry.npub) {
            return Err(RegistryError::Roster {
                name: name.clone(),
                message: format!("npub `{}`: {message}", entry.npub),
            });
        }
    }
    Ok(roster.agents)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD_NPUB: &str = "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy";

    fn write(dir: &tempfile::TempDir, content: &str) -> std::path::PathBuf {
        let path = dir.path().join("roster.toml");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn good_roster_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            &format!(
                "[agents.ruben]\nnpub = \"{GOOD_NPUB}\"\nskill_tags = [\"rust\"]\nday_rate = {{ amount = 500000000, mint = \"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v\" }}\n"
            ),
        );
        let roster = load(&path).expect("parses");
        assert_eq!(roster["ruben"].skill_tags, vec!["rust"]);
        assert_eq!(roster["ruben"].day_rate.as_ref().unwrap().amount, 500000000);
    }

    #[test]
    fn bad_npub_rejected_with_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "[agents.ruben]\nnpub = \"npub1notakey\"\n");
        let err = load(&path).unwrap_err().to_string();
        assert!(
            err.contains("ruben") && err.contains("npub1notakey"),
            "{err}"
        );
    }

    #[test]
    fn unknown_field_rejected_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            &format!("[agents.ruben]\nnpub = \"{GOOD_NPUB}\"\nday_rat = 5\n"),
        );
        let err = load(&path).unwrap_err().to_string();
        assert!(err.contains("day_rat"), "{err}");
    }
}
