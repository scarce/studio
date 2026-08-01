//! Demand-ledger rows. Pure persistence — validation happened upstream
//! (`studio-core::rfq::capture`) before anything reaches here.

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use studio_types::{Amount, Rfq};

use crate::Result;

pub async fn insert(pool: &SqlitePool, rfq: &Rfq) -> Result<()> {
    sqlx::query(
        "INSERT INTO rfqs (id, query, product, monetization, competition,
                           budget_amount, budget_mint, buyer_npub,
                           buyer_signature, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(&rfq.id)
    .bind(&rfq.query)
    .bind(&rfq.product)
    .bind(&rfq.monetization)
    .bind(serde_json::to_string(&rfq.competition).expect("Vec<String> serializes"))
    .bind(rfq.budget_ceiling.as_ref().map(|b| b.amount as i64))
    .bind(rfq.budget_ceiling.as_ref().map(|b| b.mint.clone()))
    .bind(&rfq.buyer_npub)
    .bind(&rfq.buyer_signature)
    .bind(rfq.created_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Rfq>> {
    let row = sqlx::query("SELECT * FROM rfqs WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(from_row).transpose()
}

/// All RFQs captured at or after `since`, oldest first.
pub async fn list_since(pool: &SqlitePool, since: Option<DateTime<Utc>>) -> Result<Vec<Rfq>> {
    let rows = match since {
        Some(ts) => {
            sqlx::query("SELECT * FROM rfqs WHERE created_at >= ?1 ORDER BY created_at ASC")
                .bind(ts.to_rfc3339())
                .fetch_all(pool)
                .await?
        }
        None => {
            sqlx::query("SELECT * FROM rfqs ORDER BY created_at ASC")
                .fetch_all(pool)
                .await?
        }
    };
    rows.into_iter().map(from_row).collect()
}

fn from_row(row: sqlx::sqlite::SqliteRow) -> Result<Rfq> {
    let competition: String = row.get("competition");
    let budget_amount: Option<i64> = row.get("budget_amount");
    let budget_mint: Option<String> = row.get("budget_mint");
    let created_at: String = row.get("created_at");
    Ok(Rfq {
        id: row.get("id"),
        query: row.get("query"),
        product: row.get("product"),
        monetization: row.get("monetization"),
        competition: serde_json::from_str(&competition).map_err(|e| {
            crate::StoreError::Corrupt(format!("rfqs.competition not a JSON array: {e}"))
        })?,
        budget_ceiling: match (budget_amount, budget_mint) {
            (Some(amount), Some(mint)) => Some(Amount {
                amount: amount as u64,
                mint,
            }),
            (None, None) => None,
            _ => {
                return Err(crate::StoreError::Corrupt(
                    "rfqs.budget_amount/budget_mint must be both set or both null".into(),
                ))
            }
        },
        buyer_npub: row.get("buyer_npub"),
        buyer_signature: row.get("buyer_signature"),
        created_at: DateTime::parse_from_rfc3339(&created_at)
            .map_err(|e| crate::StoreError::Corrupt(format!("rfqs.created_at: {e}")))?
            .with_timezone(&Utc),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, created_at: &str) -> Rfq {
        Rfq {
            id: id.into(),
            query: "solana priority fee forecast api".into(),
            product: Some("p50/p90 fee forecast per program id".into()),
            monetization: None,
            competition: vec!["helius fee api".into()],
            budget_ceiling: Some(Amount {
                amount: 250_000_000,
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            }),
            buyer_npub: "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy".into(),
            buyer_signature: Some("recorded-not-verified".into()),
            created_at: DateTime::parse_from_rfc3339(created_at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    #[tokio::test]
    async fn round_trips_through_sqlite() {
        let pool = crate::open("sqlite::memory:").await.unwrap();
        let rfq = sample("rfq-1", "2026-08-01T15:00:00Z");
        insert(&pool, &rfq).await.unwrap();

        let back = get(&pool, "rfq-1").await.unwrap().expect("row exists");
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&rfq).unwrap()
        );
        assert!(get(&pool, "rfq-nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_since_filters_and_orders() {
        let pool = crate::open("sqlite::memory:").await.unwrap();
        insert(&pool, &sample("a", "2026-08-01T10:00:00Z"))
            .await
            .unwrap();
        insert(&pool, &sample("b", "2026-08-01T12:00:00Z"))
            .await
            .unwrap();
        insert(&pool, &sample("c", "2026-08-01T14:00:00Z"))
            .await
            .unwrap();

        let all = list_since(&pool, None).await.unwrap();
        assert_eq!(
            all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );

        let since = DateTime::parse_from_rfc3339("2026-08-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let recent = list_since(&pool, Some(since)).await.unwrap();
        assert_eq!(
            recent.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["b", "c"]
        );
    }

    #[tokio::test]
    async fn records_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("restart.db").display());

        let pool = crate::open(&url).await.unwrap();
        insert(&pool, &sample("durable", "2026-08-01T15:00:00Z"))
            .await
            .unwrap();
        pool.close().await;

        let pool = crate::open(&url).await.unwrap();
        assert!(get(&pool, "durable").await.unwrap().is_some());
    }
}
