//! Lifecycle mirror — turns API beats into Buzz posts (PLAN.md M3, shortcut
//! per ludovic 2026-08-01: buzz visibility first, pay.sh intake later).
//!
//! demand captured / quote issued / quote accepted post to the studio ops
//! channel; acceptance additionally creates the per-project workroom channel
//! (`proj-<slug>-<shortid>`) and posts "contract starting" there. The
//! workroom's channel-create event id is stored as the FUNDED →
//! WORKROOM_ACTIVE evidence (ARCHITECTURE.md §evidence table).
//!
//! Best-effort by design in this slice: the projection row committed before
//! the beat was emitted, so a failed mirror never loses ledger state — it
//! loses a post, and logs loudly. Durable publish-then-commit arrives with
//! the substrate work (archy's replay design).

use sqlx::SqlitePool;
use studio_api::LifecycleBeat;
use studio_buzz::BuzzPort;
use studio_types::{Quote, Rfq};
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

pub fn spawn<B: BuzzPort>(
    buzz: B,
    ops_channel: Uuid,
    db: SqlitePool,
    mut rx: UnboundedReceiver<LifecycleBeat>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(beat) = rx.recv().await {
            if let Err(e) = mirror_one(&buzz, ops_channel, &db, beat).await {
                tracing::error!(error = %e, "lifecycle beat not mirrored to buzz");
            }
        }
        tracing::info!("lifecycle mirror stopped (sender closed)");
    })
}

async fn mirror_one<B: BuzzPort>(
    buzz: &B,
    ops_channel: Uuid,
    db: &SqlitePool,
    beat: LifecycleBeat,
) -> anyhow::Result<()> {
    match beat {
        LifecycleBeat::DemandCaptured { rfq } => {
            let event_id = buzz.post(ops_channel, &demand_post(&rfq)).await?;
            tracing::info!(rfq_id = %rfq.id, %event_id, "demand mirrored to ops channel");
        }
        LifecycleBeat::QuoteIssued { quote } => {
            let event_id = buzz.post(ops_channel, &quote_post(&quote)).await?;
            tracing::info!(rfq_id = %quote.rfq_id, %event_id, "quote mirrored to ops channel");
        }
        LifecycleBeat::QuoteAccepted { rfq, quote } => {
            buzz.post(ops_channel, &accepted_post(&rfq, &quote)).await?;

            // Redelivery guard (restart replay): one workroom per rfq, ever.
            if let Some(existing) = studio_store::workrooms::get_by_rfq(db, &rfq.id).await? {
                tracing::info!(rfq_id = %rfq.id, channel_id = %existing.channel_id,
                    "workroom already exists; not recreating");
                return Ok(());
            }

            let name = workroom_name(&rfq);
            let created = buzz
                .create_channel(&name, &workroom_about(&rfq, &quote))
                .await?;
            studio_store::workrooms::record(
                db,
                &studio_store::workrooms::Workroom {
                    rfq_id: rfq.id.clone(),
                    channel_id: created.channel_id.to_string(),
                    create_event_id: created.create_event_id.clone(),
                    created_at: chrono::Utc::now(),
                },
            )
            .await?;
            buzz.post(created.channel_id, &contract_post(&rfq, &quote))
                .await?;
            buzz.post(
                ops_channel,
                &format!(
                    "workroom `{name}` opened for rfq `{}` — channel {} (create event `{}`)",
                    rfq.id, created.channel_id, created.create_event_id
                ),
            )
            .await?;
            tracing::info!(rfq_id = %rfq.id, channel_id = %created.channel_id,
                create_event_id = %created.create_event_id, "workroom created — contract starting");
        }
    }
    Ok(())
}

fn workroom_about(rfq: &Rfq, quote: &Quote) -> String {
    format!(
        "Workroom for rfq {} — {} · {} (mint {}) · {}",
        rfq.id, rfq.query, quote.price.amount, quote.price.mint, quote.timeline
    )
}

/// `proj-<slug>-<shortid>`: slug from the demand query, short id for
/// uniqueness (channel names are not unique on the relay; the uuid is).
pub fn workroom_name(rfq: &Rfq) -> String {
    let slug: String = rfq
        .query
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = slug.chars().take(32).collect::<String>();
    let slug = slug.trim_end_matches('-');
    let short = rfq.id.chars().take(8).collect::<String>();
    format!("proj-{slug}-{short}")
}

fn budget_line(rfq: &Rfq) -> String {
    match &rfq.budget_ceiling {
        Some(amount) => format!("{} (mint `{}`)", amount.amount, amount.mint),
        None => "unstated".into(),
    }
}

fn demand_post(rfq: &Rfq) -> String {
    format!(
        "📥 demand captured — rfq `{}`\n> {}\nbuyer `{}` · budget {}",
        rfq.id,
        rfq.query,
        rfq.buyer_npub,
        budget_line(rfq),
    )
}

fn quote_post(quote: &Quote) -> String {
    format!(
        "💰 quote issued — rfq `{}`\nprice {} (mint `{}`) · {} milestone(s) · timeline: {}\nexpires {} · policy `{}`",
        quote.rfq_id,
        quote.price.amount,
        quote.price.mint,
        quote.milestones.len(),
        quote.timeline,
        quote.expires_at.to_rfc3339(),
        quote.policy_hash,
    )
}

fn accepted_post(rfq: &Rfq, quote: &Quote) -> String {
    format!(
        "✅ quote accepted — rfq `{}` at {}\nbuyer `{}` accepted {} (mint `{}`); funding stubbed (PLAN §6 override) — opening the workroom",
        rfq.id,
        quote
            .accepted_at
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "?".into()),
        rfq.buyer_npub,
        quote.price.amount,
        quote.price.mint,
    )
}

fn contract_post(rfq: &Rfq, quote: &Quote) -> String {
    let milestones = quote
        .milestones
        .iter()
        .enumerate()
        .map(|(i, m)| format!("{}. {} — {} ({})", i + 1, m.title, m.description, m.amount))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "🚀 contract starting — rfq `{}`\n> {}\n\nmilestones:\n{}\n\ntimeline: {} · policy `{}`\nThis channel is the workroom: demos, decisions, and delivery land here.",
        rfq.id, rfq.query, milestones, quote.timeline, quote.policy_hash,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_buzz::{MockBuzz, MockCall};
    use studio_types::{Amount, QuoteStatus};

    fn ts(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn rfq() -> Rfq {
        Rfq {
            id: "9e342a83-429b-4887-9cae-6ddecd78f7c5".into(),
            query: "Solana priority-fee forecast API!".into(),
            product: None,
            monetization: None,
            competition: vec![],
            budget_ceiling: None,
            buyer_npub: "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy".into(),
            buyer_signature: None,
            created_at: ts("2026-08-01T14:00:00Z"),
        }
    }

    fn quote() -> Quote {
        Quote {
            id: "q-1".into(),
            rfq_id: rfq().id,
            price: Amount {
                amount: 250_000_000,
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            },
            milestones: vec![studio_types::MilestoneSpec {
                title: "all of it".into(),
                description: "one milestone".into(),
                amount: 250_000_000,
            }],
            timeline: "2 weeks".into(),
            payout_destination: studio_types::PayoutDestination::Splits {
                splits: vec![studio_types::Split {
                    recipient: "CrewAgentA111111111111111111111111111111111".into(),
                    bps: 10_000,
                }],
            },
            channel: studio_types::ChannelParams {
                grace_seconds: 172_800,
                idle_timeout_seconds: 604_800,
            },
            gate_policy: studio_types::GatePolicy::studio_default(),
            policy_hash: "hash".into(),
            expires_at: ts("2026-09-01T00:00:00Z"),
            status: QuoteStatus::Accepted,
            created_at: ts("2026-08-01T15:00:00Z"),
            lapsed_at: None,
            accepted_at: Some(ts("2026-08-01T16:00:00Z")),
        }
    }

    #[test]
    fn workroom_name_is_slugged_and_bounded() {
        assert_eq!(
            workroom_name(&rfq()),
            "proj-solana-priority-fee-forecast-api-9e342a83"
        );
        let mut long = rfq();
        long.query = "x".repeat(500);
        let name = workroom_name(&long);
        assert!(name.len() <= "proj-".len() + 32 + 1 + 8);
    }

    #[tokio::test]
    async fn accepted_beat_creates_workroom_once_with_evidence() {
        let db = studio_store::open("sqlite::memory:").await.unwrap();
        studio_store::rfqs::insert(&db, &rfq()).await.unwrap();
        let buzz = MockBuzz::default();
        let ops = Uuid::from_u128(999);

        mirror_one(
            &buzz,
            ops,
            &db,
            LifecycleBeat::QuoteAccepted {
                rfq: Box::new(rfq()),
                quote: Box::new(quote()),
            },
        )
        .await
        .unwrap();

        let calls = buzz.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 4, "{calls:#?}");
        assert!(matches!(&calls[0], MockCall::Post { channel_id, .. } if *channel_id == ops));
        assert!(
            matches!(&calls[1], MockCall::CreateChannel { name, .. } if name.starts_with("proj-solana-priority"))
        );
        // contract post lands in the NEW channel, not ops
        assert!(matches!(&calls[2], MockCall::Post { channel_id, .. } if *channel_id != ops));
        assert!(matches!(&calls[3], MockCall::Post { channel_id, .. } if *channel_id == ops));

        // evidence row: channel-create event id recorded
        let workroom = studio_store::workrooms::get_by_rfq(&db, &rfq().id)
            .await
            .unwrap()
            .expect("workroom recorded");
        assert_eq!(workroom.create_event_id, "mock-create-event-2");

        // redelivery: no second channel
        drop(calls);
        mirror_one(
            &buzz,
            ops,
            &db,
            LifecycleBeat::QuoteAccepted {
                rfq: Box::new(rfq()),
                quote: Box::new(quote()),
            },
        )
        .await
        .unwrap();
        let calls = buzz.calls.lock().unwrap();
        assert_eq!(calls.len(), 5, "only the ops accepted-post repeats");
        assert!(matches!(&calls[4], MockCall::Post { .. }));
    }

    #[tokio::test]
    async fn demand_and_quote_beats_post_to_ops() {
        let db = studio_store::open("sqlite::memory:").await.unwrap();
        let buzz = MockBuzz::default();
        let ops = Uuid::from_u128(999);

        mirror_one(
            &buzz,
            ops,
            &db,
            LifecycleBeat::DemandCaptured {
                rfq: Box::new(rfq()),
            },
        )
        .await
        .unwrap();
        mirror_one(
            &buzz,
            ops,
            &db,
            LifecycleBeat::QuoteIssued {
                quote: Box::new(quote()),
            },
        )
        .await
        .unwrap();

        let calls = buzz.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        for call in calls.iter() {
            assert!(matches!(call, MockCall::Post { channel_id, .. } if *channel_id == ops));
        }
    }
}
