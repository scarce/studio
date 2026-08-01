//! Operator one-offs against a community relay, speaking as the studio
//! identity. Kept as an example so it never ships in the daemon.
//!
//! ```sh
//! export STUDIO_KEY=<nsec-or-hex>   # and STUDIO_AUTH_TAG for managed identities
//! cargo run -p studio-buzz --example channel_admin -- \
//!     wss://scarce.communities.buzz.xyz set-visibility <channel-uuid> private
//! cargo run -p studio-buzz --example channel_admin -- \
//!     wss://scarce.communities.buzz.xyz add-member <channel-uuid> <npub-or-hex>
//! ```

use studio_buzz::{BuzzPort, RelayBuzz};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    studio_buzz::install_crypto_provider();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let key = std::env::var("STUDIO_KEY").map_err(|_| "STUDIO_KEY env var is required")?;
    let auth_tag = std::env::var("STUDIO_AUTH_TAG").ok();

    let usage = "usage: channel_admin <relay-url> set-visibility <channel> <open|private> | add-member <channel> <npub-or-hex>";
    let [relay, cmd, channel, value] = args.as_slice() else {
        return Err(usage.into());
    };
    let channel = uuid::Uuid::parse_str(channel)?;
    let buzz = RelayBuzz::new(relay, &key, auth_tag.as_deref())?;

    let event_id = match cmd.as_str() {
        "set-visibility" => buzz.set_visibility(channel, value).await?,
        "add-member" => {
            buzz.add_member(channel, &studio_buzz::pubkey_hex(value)?)
                .await?
        }
        _ => return Err(usage.into()),
    };
    println!("{{\"accepted\":true,\"event_id\":\"{event_id}\"}}");
    Ok(())
}
