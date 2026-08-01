//! Quote rows. Pure persistence — validation happened upstream
//! (`studio-core::quote::issue`) before anything reaches here. One quote per
//! RFQ in v0 (see migration 0004); the singleton is enforced by the primary
//! key, surfaced as `StoreError::Conflict`.

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use studio_types::{Amount, Quote, QuoteStatus};

use crate::{Result, StoreError};

pub async fn insert(pool: &SqlitePool, quote: &Quote) -> Result<()> {
    let result = sqlx::query(
        "INSERT INTO quotes (rfq_id, id, price_amount, price_mint, milestones,
                             timeline, payout_destination, grace_seconds,
                             idle_timeout_seconds, gate_policy, policy_hash,
                             expires_at, status, created_at, lapsed_at,
                             accepted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )
    .bind(&quote.rfq_id)
    .bind(&quote.id)
    .bind(quote.price.amount as i64)
    .bind(&quote.price.mint)
    .bind(serde_json::to_string(&quote.milestones).expect("milestones serialize"))
    .bind(&quote.timeline)
    .bind(serde_json::to_string(&quote.payout_destination).expect("payout serializes"))
    .bind(quote.channel.grace_seconds as i64)
    .bind(quote.channel.idle_timeout_seconds as i64)
    .bind(serde_json::to_string(&quote.gate_policy).expect("gate policy serializes"))
    .bind(&quote.policy_hash)
    .bind(quote.expires_at.to_rfc3339())
    .bind(status_str(quote.status))
    .bind(quote.created_at.to_rfc3339())
    .bind(quote.lapsed_at.map(|t| t.to_rfc3339()))
    .bind(quote.accepted_at.map(|t| t.to_rfc3339()))
    .execute(pool)
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(StoreError::Conflict(
            format!("a quote already exists for rfq `{}`", quote.rfq_id),
        )),
        Err(e) => Err(e.into()),
    }
}

pub async fn get_by_rfq(pool: &SqlitePool, rfq_id: &str) -> Result<Option<Quote>> {
    let row = sqlx::query("SELECT * FROM quotes WHERE rfq_id = ?1")
        .bind(rfq_id)
        .fetch_optional(pool)
        .await?;
    row.map(from_row).transpose()
}

/// Stamp LAPSED on every QUOTED row whose expiry has passed. Returns the
/// number of quotes lapsed. Reads are already fail-closed against sweep lag
/// (`Quote::at` derives LAPSED past expiry); the sweep makes the projection
/// row itself catch up.
pub async fn sweep_lapsed(pool: &SqlitePool, now: DateTime<Utc>) -> Result<u64> {
    let now = now.to_rfc3339();
    let result = sqlx::query(
        "UPDATE quotes SET status = 'LAPSED', lapsed_at = ?1
         WHERE status = 'QUOTED' AND expires_at <= ?1",
    )
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Stamp ACCEPTED, atomically guarded: only a live QUOTED row can accept.
/// Returns the number of rows updated — 0 means the quote was already
/// accepted, already lapsed, or past expiry (the caller re-derives which for
/// its error message); the guard makes double-accept a lost race, not a
/// second acceptance.
pub async fn mark_accepted(pool: &SqlitePool, rfq_id: &str, now: DateTime<Utc>) -> Result<u64> {
    let now = now.to_rfc3339();
    let result = sqlx::query(
        "UPDATE quotes SET status = 'ACCEPTED', accepted_at = ?1
         WHERE rfq_id = ?2 AND status = 'QUOTED' AND expires_at > ?1",
    )
    .bind(&now)
    .bind(rfq_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

fn status_str(status: QuoteStatus) -> &'static str {
    match status {
        QuoteStatus::Quoted => "QUOTED",
        QuoteStatus::Lapsed => "LAPSED",
        QuoteStatus::Accepted => "ACCEPTED",
    }
}

fn corrupt(what: &str, detail: String) -> StoreError {
    StoreError::Corrupt(format!("quotes.{what}: {detail}"))
}

fn json<T: serde::de::DeserializeOwned>(what: &'static str, raw: String) -> Result<T> {
    serde_json::from_str(&raw).map_err(|e| corrupt(what, e.to_string()))
}

fn ts(what: &'static str, raw: String) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&raw)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| corrupt(what, e.to_string()))
}

fn from_row(row: sqlx::sqlite::SqliteRow) -> Result<Quote> {
    let status: String = row.get("status");
    let lapsed_at: Option<String> = row.get("lapsed_at");
    let accepted_at: Option<String> = row.get("accepted_at");
    Ok(Quote {
        id: row.get("id"),
        rfq_id: row.get("rfq_id"),
        price: Amount {
            amount: row.get::<i64, _>("price_amount") as u64,
            mint: row.get("price_mint"),
        },
        milestones: json("milestones", row.get("milestones"))?,
        timeline: row.get("timeline"),
        payout_destination: json("payout_destination", row.get("payout_destination"))?,
        channel: studio_types::ChannelParams {
            grace_seconds: row.get::<i64, _>("grace_seconds") as u64,
            idle_timeout_seconds: row.get::<i64, _>("idle_timeout_seconds") as u64,
        },
        gate_policy: json("gate_policy", row.get("gate_policy"))?,
        policy_hash: row.get("policy_hash"),
        expires_at: ts("expires_at", row.get("expires_at"))?,
        status: match status.as_str() {
            "QUOTED" => QuoteStatus::Quoted,
            "LAPSED" => QuoteStatus::Lapsed,
            "ACCEPTED" => QuoteStatus::Accepted,
            other => return Err(corrupt("status", format!("unknown status `{other}`"))),
        },
        created_at: ts("created_at", row.get("created_at"))?,
        lapsed_at: lapsed_at.map(|raw| ts("lapsed_at", raw)).transpose()?,
        accepted_at: accepted_at.map(|raw| ts("accepted_at", raw)).transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_types::{ChannelParams, GatePolicy, MilestoneSpec, PayoutDestination, Rfq, Split};

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    async fn seeded_pool() -> SqlitePool {
        let pool = crate::open("sqlite::memory:").await.unwrap();
        // quotes reference rfqs; seed the parent rows.
        for id in ["rfq-1", "rfq-2"] {
            crate::rfqs::insert(
                &pool,
                &Rfq {
                    id: id.into(),
                    query: "solana priority fee forecast api".into(),
                    product: None,
                    monetization: None,
                    competition: vec![],
                    budget_ceiling: None,
                    buyer_npub: "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy"
                        .into(),
                    buyer_signature: None,
                    created_at: ts("2026-08-01T14:00:00Z"),
                },
            )
            .await
            .unwrap();
        }
        pool
    }

    fn sample(id: &str, rfq_id: &str, expires_at: &str) -> Quote {
        let gate_policy = GatePolicy::studio_default();
        Quote {
            id: id.into(),
            rfq_id: rfq_id.into(),
            price: Amount {
                amount: 250_000_000,
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            },
            milestones: vec![
                MilestoneSpec {
                    title: "Forecast model".into(),
                    description: "p50/p90 per program id".into(),
                    amount: 150_000_000,
                },
                MilestoneSpec {
                    title: "Gated endpoint".into(),
                    description: "pay.sh-gated REST endpoint".into(),
                    amount: 100_000_000,
                },
            ],
            timeline: "2 weeks, weekly demos".into(),
            payout_destination: PayoutDestination::Splits {
                splits: vec![Split {
                    recipient: "CrewAgentA111111111111111111111111111111111".into(),
                    bps: 10_000,
                }],
            },
            channel: ChannelParams {
                grace_seconds: 172_800,
                idle_timeout_seconds: 604_800,
            },
            policy_hash: "policy-hash-set-at-issue-time".into(),
            gate_policy,
            expires_at: ts(expires_at),
            status: QuoteStatus::Quoted,
            created_at: ts("2026-08-01T15:00:00Z"),
            lapsed_at: None,
            accepted_at: None,
        }
    }

    #[tokio::test]
    async fn round_trips_through_sqlite() {
        let pool = seeded_pool().await;
        let quote = sample("q-1", "rfq-1", "2026-08-08T15:00:00Z");
        insert(&pool, &quote).await.unwrap();

        let back = get_by_rfq(&pool, "rfq-1")
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&quote).unwrap()
        );
        assert!(get_by_rfq(&pool, "rfq-2").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn second_quote_for_the_same_rfq_conflicts() {
        let pool = seeded_pool().await;
        insert(&pool, &sample("q-1", "rfq-1", "2026-08-08T15:00:00Z"))
            .await
            .unwrap();
        let err = insert(&pool, &sample("q-2", "rfq-1", "2026-08-09T15:00:00Z"))
            .await
            .unwrap_err();
        assert!(matches!(err, StoreError::Conflict(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn sweep_lapses_exactly_the_expired_quoted_rows() {
        let pool = seeded_pool().await;
        insert(&pool, &sample("q-1", "rfq-1", "2026-08-02T00:00:00Z"))
            .await
            .unwrap();
        insert(&pool, &sample("q-2", "rfq-2", "2026-08-09T00:00:00Z"))
            .await
            .unwrap();

        // before either expiry: nothing to do
        assert_eq!(
            sweep_lapsed(&pool, ts("2026-08-01T23:59:59Z"))
                .await
                .unwrap(),
            0
        );

        // past q-1's expiry only
        let now = ts("2026-08-02T00:00:30Z");
        assert_eq!(sweep_lapsed(&pool, now).await.unwrap(), 1);

        let lapsed = get_by_rfq(&pool, "rfq-1").await.unwrap().unwrap();
        assert_eq!(lapsed.status, QuoteStatus::Lapsed);
        assert_eq!(lapsed.lapsed_at, Some(now));

        let live = get_by_rfq(&pool, "rfq-2").await.unwrap().unwrap();
        assert_eq!(live.status, QuoteStatus::Quoted);
        assert_eq!(live.lapsed_at, None);

        // idempotent: an already-lapsed row is not re-stamped
        assert_eq!(
            sweep_lapsed(&pool, ts("2026-08-02T01:00:00Z"))
                .await
                .unwrap(),
            0
        );
        let unchanged = get_by_rfq(&pool, "rfq-1").await.unwrap().unwrap();
        assert_eq!(unchanged.lapsed_at, Some(now));
    }

    #[tokio::test]
    async fn accept_stamps_once_and_only_live_quoted_rows() {
        let pool = seeded_pool().await;
        insert(&pool, &sample("q-1", "rfq-1", "2026-08-08T15:00:00Z"))
            .await
            .unwrap();
        insert(&pool, &sample("q-2", "rfq-2", "2026-08-02T00:00:00Z"))
            .await
            .unwrap();

        let now = ts("2026-08-01T16:00:00Z");
        assert_eq!(mark_accepted(&pool, "rfq-1", now).await.unwrap(), 1);
        let accepted = get_by_rfq(&pool, "rfq-1").await.unwrap().unwrap();
        assert_eq!(accepted.status, QuoteStatus::Accepted);
        assert_eq!(accepted.accepted_at, Some(now));

        // double-accept loses the guard
        assert_eq!(mark_accepted(&pool, "rfq-1", now).await.unwrap(), 0);

        // past expiry: no acceptance, even before the sweep stamps LAPSED
        let late = ts("2026-08-02T00:00:30Z");
        assert_eq!(mark_accepted(&pool, "rfq-2", late).await.unwrap(), 0);

        // the sweep never touches an accepted row
        assert_eq!(
            sweep_lapsed(&pool, ts("2026-09-01T00:00:00Z"))
                .await
                .unwrap(),
            1
        );
        let still = get_by_rfq(&pool, "rfq-1").await.unwrap().unwrap();
        assert_eq!(still.status, QuoteStatus::Accepted);
        assert_eq!(still.lapsed_at, None);
    }
}
