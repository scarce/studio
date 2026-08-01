//! `BuzzPort` — the studio's hands on the coordination substrate.
//!
//! Trait + crate-backed impl (buzz-sdk builders + buzz-ws-client transport,
//! PLAN.md §1) + a recording mock. The orchestrator only ever sees the
//! trait, so the lifecycle mirror is testable without a relay.
//!
//! The relay impl signs with the studio key and publishes over NIP-42
//! authenticated websocket, one connection per publish — the mirror's volume
//! is a handful of events per engagement, so connection reuse buys nothing
//! yet. Every publish waits for the relay's OK: an event id this crate
//! returns is one the relay has accepted, which is what lets callers store
//! it as transition evidence (ARCHITECTURE.md §1).

use std::future::Future;

use nostr::{EventBuilder, Keys, Tag};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum BuzzError {
    #[error("invalid studio key: {0}")]
    Key(String),
    #[error("invalid auth tag: {0}")]
    AuthTag(String),
    #[error("event build failed: {0}")]
    Build(String),
    #[error("relay transport: {0}")]
    Transport(#[from] buzz_ws_client::WsClientError),
    #[error("relay rejected event {event_id}: {message}")]
    Rejected { event_id: String, message: String },
}

/// A channel the port created, with the relay-accepted create event id —
/// the FUNDED → WORKROOM_ACTIVE evidence when the channel is a workroom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedChannel {
    pub channel_id: Uuid,
    pub create_event_id: String,
}

pub trait BuzzPort: Send + Sync + 'static {
    /// Create an open stream channel; returns ids only after the relay OK.
    fn create_channel(
        &self,
        name: &str,
        about: &str,
    ) -> impl Future<Output = Result<CreatedChannel, BuzzError>> + Send;

    /// Post a message into a channel; returns the relay-accepted event id.
    fn post(
        &self,
        channel_id: Uuid,
        content: &str,
    ) -> impl Future<Output = Result<String, BuzzError>> + Send;
}

/// The real thing: studio key + community relay.
pub struct RelayBuzz {
    relay_url: String,
    keys: Keys,
    /// NIP-OA authorization tag (owner-granted membership). Required for
    /// managed-agent identities; rides the AUTH event and every published
    /// event, matching the buzz CLI's behavior.
    auth_tag: Option<Tag>,
}

impl RelayBuzz {
    /// `private_key` is hex or nsec; `auth_tag` is the NIP-OA tag JSON
    /// (`BUZZ_AUTH_TAG` shape). No connection happens here — each publish
    /// dials, authenticates (NIP-42), publishes, and hangs up.
    pub fn new(
        relay_url: &str,
        private_key: &str,
        auth_tag: Option<&str>,
    ) -> Result<Self, BuzzError> {
        let keys = Keys::parse(private_key).map_err(|e| BuzzError::Key(e.to_string()))?;
        let auth_tag = auth_tag
            .map(|json| {
                buzz_sdk::nip_oa::parse_auth_tag(json)
                    .map_err(|e| BuzzError::AuthTag(e.to_string()))
            })
            .transpose()?;
        Ok(Self {
            relay_url: relay_url.to_string(),
            keys,
            auth_tag,
        })
    }

    /// The studio identity's public key (hex) — logged at startup so an
    /// operator can add it to the community before the first publish.
    pub fn public_key_hex(&self) -> String {
        self.keys.public_key().to_hex()
    }

    async fn publish(&self, builder: EventBuilder) -> Result<String, BuzzError> {
        let builder = match &self.auth_tag {
            Some(tag) => builder.tags([tag.clone()]),
            None => builder,
        };
        let event = builder
            .sign_with_keys(&self.keys)
            .map_err(|e| BuzzError::Build(e.to_string()))?;
        let event_id = event.id.to_hex();

        let mut conn = buzz_ws_client::NostrWsConnection::connect_authenticated(
            &self.relay_url,
            &self.keys,
            self.auth_tag.as_ref(),
        )
        .await?;
        let ok = conn.send_event(event).await?;
        // Best-effort close; the OK already landed.
        let _ = conn.disconnect().await;

        if !ok.accepted {
            return Err(BuzzError::Rejected {
                event_id,
                message: ok.message,
            });
        }
        Ok(event_id)
    }
}

impl BuzzPort for RelayBuzz {
    async fn create_channel(&self, name: &str, about: &str) -> Result<CreatedChannel, BuzzError> {
        let channel_id = Uuid::new_v4();
        let builder = buzz_sdk::build_create_channel(
            channel_id,
            name,
            Some(buzz_sdk::Visibility::Open),
            Some(buzz_sdk::ChannelKind::Stream),
            Some(about),
            None,
        )
        .map_err(|e| BuzzError::Build(e.to_string()))?;
        let create_event_id = self.publish(builder).await?;
        Ok(CreatedChannel {
            channel_id,
            create_event_id,
        })
    }

    async fn post(&self, channel_id: Uuid, content: &str) -> Result<String, BuzzError> {
        let builder = buzz_sdk::build_message(channel_id, content, None, &[], false, &[])
            .map_err(|e| BuzzError::Build(e.to_string()))?;
        self.publish(builder).await
    }
}

/// Recording mock for orchestrator tests: deterministic ids, captured calls.
#[derive(Default)]
pub struct MockBuzz {
    pub calls: std::sync::Mutex<Vec<MockCall>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockCall {
    CreateChannel { name: String, about: String },
    Post { channel_id: Uuid, content: String },
}

impl BuzzPort for MockBuzz {
    async fn create_channel(&self, name: &str, about: &str) -> Result<CreatedChannel, BuzzError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(MockCall::CreateChannel {
            name: name.to_string(),
            about: about.to_string(),
        });
        let n = calls.len();
        Ok(CreatedChannel {
            channel_id: Uuid::from_u128(n as u128),
            create_event_id: format!("mock-create-event-{n}"),
        })
    }

    async fn post(&self, channel_id: Uuid, content: &str) -> Result<String, BuzzError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(MockCall::Post {
            channel_id,
            content: content.to_string(),
        });
        Ok(format!("mock-post-event-{}", calls.len()))
    }
}
