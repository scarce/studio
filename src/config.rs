//! Configuration: defaults ← YAML file (`--config`) ← `SCARCED_*` env.
//! Nested keys join with `__` in env form (`SCARCED_BUZZ__RELAY_URL`).
//! Secrets stay out of the repo — the studio keys arrive in M3 and come from
//! the environment via the deployment's secret manager, never from files
//! checked in here (`scarced.example.yaml` carries dev values only).

use std::path::Path;

use figment::{
    providers::{Env, Format, Serialized, Yaml},
    Figment,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Socket address the HTTP surface binds, e.g. `127.0.0.1:7380`.
    pub bind: String,
    /// SQLite URL of the projection store, e.g. `sqlite://scarced.db`.
    /// Created if missing; droppable and rebuildable by design.
    pub db: String,
    /// Bearer token for studio-authenticated routes (quote issuance).
    /// Unset or empty disables those routes — fail-closed, never a default
    /// credential.
    #[serde(default)]
    pub studio_token: Option<String>,
    /// Quote-expiry sweep cadence, seconds. Reads are fail-closed against
    /// sweep lag either way; the sweep keeps the projection rows honest.
    pub sweep_seconds: u64,
    /// Studio repo root holding `agents/*.persona.md`, `roster.toml` and
    /// `skills.toml` (GUIDELINES.md §3–§4). Loaded fail-closed at boot: a
    /// registry that does not parse is a scarced that does not start.
    pub registry_dir: String,
    /// Buzz community the studio operates in. Recorded now, consumed by the
    /// M3 orchestrator (PLAN.md M3).
    #[serde(default)]
    pub buzz: Option<BuzzConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuzzConfig {
    /// Community relay websocket URL, e.g. `wss://scarce.communities.buzz.xyz`.
    pub relay_url: String,
    /// Studio identity key (hex or nsec) the daemon signs with. Prefer the
    /// env form (`SCARCED_BUZZ__PRIVATE_KEY`) outside dev.
    pub private_key: String,
    /// Ops channel uuid — the lifecycle mirror posts demand/quote/accept
    /// beats here. Workroom channels are created per accepted engagement.
    pub ops_channel: String,
    /// NIP-OA auth tag JSON (`BUZZ_AUTH_TAG` shape). Required when the
    /// studio key is a managed-agent identity; owner-key runs omit it.
    #[serde(default)]
    pub auth_tag: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:7380".into(),
            db: "sqlite://scarced.db".into(),
            studio_token: None,
            sweep_seconds: 30,
            registry_dir: ".".into(),
            buzz: None,
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        let mut figment = Figment::from(Serialized::defaults(Config::default()));
        if let Some(path) = path {
            anyhow::ensure!(path.is_file(), "config file not found: {}", path.display());
            figment = figment.merge(Yaml::file_exact(path));
        }
        let mut config: Config = figment
            .merge(
                Env::prefixed("SCARCED_")
                    .split("__")
                    // The auth tag value is JSON; figment's lenient env
                    // parsing would decode it into a sequence and fail the
                    // string field. It bypasses figment below, read raw.
                    .ignore(&["buzz.auth_tag", "buzz__auth_tag"]),
            )
            .extract()
            .map_err(|e| anyhow::anyhow!("invalid config: {e}"))?;

        if let (Some(buzz), Ok(raw)) = (
            config.buzz.as_mut(),
            std::env::var("SCARCED_BUZZ__AUTH_TAG"),
        ) {
            if !raw.trim().is_empty() {
                buzz.auth_tag = Some(raw);
            }
        }

        config.studio_token = config.studio_token.filter(|t| !t.trim().is_empty());
        anyhow::ensure!(
            config.sweep_seconds > 0,
            "sweep_seconds (SCARCED_SWEEP_SECONDS) must be a positive integer"
        );
        anyhow::ensure!(
            !config.registry_dir.trim().is_empty(),
            "registry_dir (SCARCED_REGISTRY_DIR) must point at the studio repo root"
        );
        if let Some(buzz) = &config.buzz {
            anyhow::ensure!(
                buzz.relay_url.starts_with("wss://") || buzz.relay_url.starts_with("ws://"),
                "buzz.relay_url must be a ws:// or wss:// URL, got `{}`",
                buzz.relay_url
            );
            anyhow::ensure!(
                !buzz.private_key.trim().is_empty(),
                "buzz.private_key (SCARCED_BUZZ__PRIVATE_KEY) must be set when buzz is configured"
            );
            anyhow::ensure!(
                uuid::Uuid::parse_str(&buzz.ops_channel).is_ok(),
                "buzz.ops_channel must be a channel uuid, got `{}`",
                buzz.ops_channel
            );
        }
        Ok(config)
    }
}

#[cfg(test)]
// figment::Jail's closure signature returns its large Error type by design.
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;

    #[test]
    fn defaults_without_file_or_env() {
        figment::Jail::expect_with(|_| {
            let config = Config::load(None).expect("defaults load");
            assert_eq!(config.bind, "127.0.0.1:7380");
            assert_eq!(config.db, "sqlite://scarced.db");
            assert_eq!(config.studio_token, None);
            assert_eq!(config.sweep_seconds, 30);
            assert_eq!(config.registry_dir, ".");
            assert!(config.buzz.is_none());
            Ok(())
        });
    }

    #[test]
    fn yaml_sets_and_env_overrides() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "scarced.yaml",
                "bind: 0.0.0.0:9999\nstudio_token: from-yaml\nbuzz:\n  relay_url: wss://example.communities.buzz.xyz\n  private_key: from-yaml-key\n  ops_channel: 8f99f8e4-ae12-4397-bc65-7b0f8a69688f\n",
            )?;
            jail.set_env("SCARCED_STUDIO_TOKEN", "from-env");
            jail.set_env("SCARCED_BUZZ__RELAY_URL", "wss://override.example");
            // JSON array value: must arrive as the raw string, not a
            // figment-parsed sequence.
            jail.set_env(
                "SCARCED_BUZZ__AUTH_TAG",
                r#"["auth","aa","{\"cap\":1}","sig"]"#,
            );
            let config = Config::load(Some(Path::new("scarced.yaml"))).expect("load");
            assert_eq!(config.bind, "0.0.0.0:9999"); // yaml over default
            assert_eq!(config.studio_token.as_deref(), Some("from-env")); // env over yaml
            let buzz = config.buzz.unwrap();
            assert_eq!(buzz.relay_url, "wss://override.example");
            assert_eq!(buzz.private_key, "from-yaml-key");
            assert_eq!(
                buzz.auth_tag.as_deref(),
                Some(r#"["auth","aa","{\"cap\":1}","sig"]"#)
            );
            assert_eq!(config.sweep_seconds, 30); // default survives partial yaml
            Ok(())
        });
    }

    #[test]
    fn missing_file_is_an_error() {
        figment::Jail::expect_with(|_| {
            let err = Config::load(Some(Path::new("nope.yaml"))).unwrap_err();
            assert!(err.to_string().contains("nope.yaml"), "{err}");
            Ok(())
        });
    }

    #[test]
    fn unknown_key_is_rejected_by_name() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("scarced.yaml", "bindd: 1.2.3.4:80\n")?;
            let err = Config::load(Some(Path::new("scarced.yaml"))).unwrap_err();
            assert!(err.to_string().contains("bindd"), "{err}");
            Ok(())
        });
    }

    #[test]
    fn zero_sweep_rejected_and_empty_token_disables() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("scarced.yaml", "sweep_seconds: 0\n")?;
            let err = Config::load(Some(Path::new("scarced.yaml"))).unwrap_err();
            assert!(err.to_string().contains("sweep_seconds"), "{err}");

            jail.set_env("SCARCED_STUDIO_TOKEN", "  ");
            let config = Config::load(None).expect("load");
            assert_eq!(config.studio_token, None);
            Ok(())
        });
    }

    #[test]
    fn non_websocket_relay_url_rejected() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "scarced.yaml",
                "buzz:\n  relay_url: https://not-a-relay\n  private_key: k\n  ops_channel: 8f99f8e4-ae12-4397-bc65-7b0f8a69688f\n",
            )?;
            let err = Config::load(Some(Path::new("scarced.yaml"))).unwrap_err();
            assert!(err.to_string().contains("relay_url"), "{err}");
            Ok(())
        });
    }

    #[test]
    fn buzz_section_requires_key_and_channel_uuid() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "scarced.yaml",
                "buzz:\n  relay_url: wss://r\n  private_key: \" \"\n  ops_channel: 8f99f8e4-ae12-4397-bc65-7b0f8a69688f\n",
            )?;
            let err = Config::load(Some(Path::new("scarced.yaml"))).unwrap_err();
            assert!(err.to_string().contains("private_key"), "{err}");

            jail.create_file(
                "scarced2.yaml",
                "buzz:\n  relay_url: wss://r\n  private_key: k\n  ops_channel: not-a-uuid\n",
            )?;
            let err = Config::load(Some(Path::new("scarced2.yaml"))).unwrap_err();
            assert!(err.to_string().contains("ops_channel"), "{err}");
            Ok(())
        });
    }
}
