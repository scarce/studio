//! Relay invite minting — `POST /api/invites`, NIP-98 signed.
//!
//! The relay serves a complete onboarding page at
//! `https://<host>/invite/<code>` (Buzz Desktop deep link, platform-aware
//! download, in-browser claim), so a minted invite URL is the smartest thing
//! a project page can hand a visitor. Minting requires the signing key to
//! hold the `owner` or `admin` role in the community — a plain member gets
//! 403, which callers must treat as "run without invites", not a crash.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag};
use sha2::{Digest, Sha256};

use crate::BuzzError;

/// A minted invite: the shareable landing-page URL and the bare code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedInvite {
    /// `https://<host>/invite/<code>` — what a page links to.
    pub url: String,
    pub code: String,
}

/// `wss://host` → `https://host`, `ws://host` → `http://host` — the relay's
/// HTTP API lives on the same authority as its websocket.
pub fn api_base(relay_url: &str) -> String {
    let relay_url = relay_url.trim_end_matches('/');
    if let Some(host) = relay_url.strip_prefix("wss://") {
        format!("https://{host}")
    } else if let Some(host) = relay_url.strip_prefix("ws://") {
        format!("http://{host}")
    } else {
        relay_url.to_string()
    }
}

/// Sign a NIP-98 HTTP auth event (kind 27235) and return the
/// `Authorization` header value — same shape as the buzz CLI: `u`, `method`
/// and `nonce` tags, plus `payload` (SHA-256 hex) when a body rides along.
pub fn nip98_header(
    keys: &Keys,
    method: &str,
    url: &str,
    body: Option<&[u8]>,
) -> Result<String, BuzzError> {
    let tag = |parts: [&str; 2]| Tag::parse(parts).map_err(|e| BuzzError::Build(e.to_string()));
    let mut tags = vec![
        tag(["u", url])?,
        tag(["method", method])?,
        // Nonce prevents replay rejection for rapid-fire identical requests.
        tag(["nonce", &uuid::Uuid::new_v4().to_string()])?,
    ];
    if let Some(body) = body {
        let hash = format!("{:x}", Sha256::digest(body));
        tags.push(tag(["payload", &hash])?);
    }
    let event = EventBuilder::new(Kind::Custom(27235), "")
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|e| BuzzError::Build(format!("NIP-98 signing failed: {e}")))?;
    Ok(format!("Nostr {}", B64.encode(event.as_json().as_bytes())))
}

pub(crate) async fn mint(
    relay_url: &str,
    keys: &Keys,
    auth_tag: Option<&Tag>,
    ttl_secs: u64,
    max_uses: Option<i32>,
) -> Result<MintedInvite, BuzzError> {
    let url = format!("{}/api/invites", api_base(relay_url));
    let body = serde_json::to_vec(&serde_json::json!({
        "ttl_secs": ttl_secs,
        "max_uses": max_uses,
    }))
    .expect("literal json serializes");

    let auth = nip98_header(keys, "POST", &url, Some(&body))?;
    let client = reqwest::Client::new();
    let mut request = client
        .post(&url)
        .header("Authorization", auth)
        .header("Content-Type", "application/json")
        .body(body);
    if let Some(tag) = auth_tag {
        // Same header the buzz CLI sends: managed identities carry their
        // NIP-OA capability on HTTP API calls too.
        let json = serde_json::to_string(tag)
            .map_err(|e| BuzzError::AuthTag(format!("auth tag does not serialize: {e}")))?;
        request = request.header("x-auth-tag", json);
    }

    let response = request
        .send()
        .await
        .map_err(|e| BuzzError::Api(format!("POST {url}: {e}")))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| BuzzError::Api(format!("POST {url}: reading body: {e}")))?;
    if !status.is_success() {
        return Err(BuzzError::Api(format!(
            "POST {url} -> {status}: {} (minting requires the studio key to be a community owner/admin)",
            text.trim()
        )));
    }
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| BuzzError::Api(format!("POST {url}: non-JSON body: {e}")))?;
    match (json["url"].as_str(), json["code"].as_str()) {
        (Some(url), Some(code)) => Ok(MintedInvite {
            url: url.to_string(),
            code: code.to_string(),
        }),
        _ => Err(BuzzError::Api(format!(
            "POST {url}: response missing url/code: {text}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_base_maps_ws_schemes() {
        assert_eq!(
            api_base("wss://scarce.communities.buzz.xyz/"),
            "https://scarce.communities.buzz.xyz"
        );
        assert_eq!(api_base("ws://localhost:3000"), "http://localhost:3000");
    }

    #[test]
    fn nip98_header_is_a_signed_27235_with_the_right_tags() {
        let keys = Keys::generate();
        let url = "https://relay.example/api/invites";
        let body = br#"{"ttl_secs":259200}"#;
        let header = nip98_header(&keys, "POST", url, Some(body)).unwrap();

        let encoded = header.strip_prefix("Nostr ").expect("Nostr scheme");
        let event = nostr::Event::from_json(B64.decode(encoded).unwrap()).unwrap();
        event.verify().expect("valid signature");
        assert_eq!(event.kind, Kind::Custom(27235));
        assert_eq!(event.pubkey, keys.public_key());

        let tag_value = |name: &str| {
            event
                .tags
                .iter()
                .find(|t| t.as_slice()[0] == name)
                .map(|t| t.as_slice()[1].clone())
        };
        assert_eq!(tag_value("u").as_deref(), Some(url));
        assert_eq!(tag_value("method").as_deref(), Some("POST"));
        assert_eq!(
            tag_value("payload").as_deref(),
            Some(format!("{:x}", Sha256::digest(body)).as_str())
        );
        assert!(tag_value("nonce").is_some());
    }
}
