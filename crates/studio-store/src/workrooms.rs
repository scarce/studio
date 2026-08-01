//! Workroom rows — the projection of the FUNDED → WORKROOM_ACTIVE
//! transition. `create_event_id` is the Buzz channel-create event id: the
//! transition's evidence (ARCHITECTURE.md §evidence table), which is why the
//! row is written only after the relay accepted the event.

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

use crate::{Result, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workroom {
    pub rfq_id: String,
    pub channel_id: String,
    pub create_event_id: String,
    pub created_at: DateTime<Utc>,
}

pub async fn record(pool: &SqlitePool, workroom: &Workroom) -> Result<()> {
    let result = sqlx::query(
        "INSERT INTO workrooms (rfq_id, channel_id, create_event_id, created_at)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(&workroom.rfq_id)
    .bind(&workroom.channel_id)
    .bind(&workroom.create_event_id)
    .bind(workroom.created_at.to_rfc3339())
    .execute(pool)
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(StoreError::Conflict(
            format!("a workroom already exists for rfq `{}`", workroom.rfq_id),
        )),
        Err(e) => Err(e.into()),
    }
}

pub async fn get_by_rfq(pool: &SqlitePool, rfq_id: &str) -> Result<Option<Workroom>> {
    let row = sqlx::query("SELECT * FROM workrooms WHERE rfq_id = ?1")
        .bind(rfq_id)
        .fetch_optional(pool)
        .await?;
    row.map(|row| {
        let raw: String = row.get("created_at");
        let created_at = DateTime::parse_from_rfc3339(&raw)
            .map(|t| t.with_timezone(&Utc))
            .map_err(|e| StoreError::Corrupt(format!("workrooms.created_at: {e}")))?;
        Ok(Workroom {
            rfq_id: row.get("rfq_id"),
            channel_id: row.get("channel_id"),
            create_event_id: row.get("create_event_id"),
            created_at,
        })
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    async fn seeded_pool() -> SqlitePool {
        let pool = crate::open("sqlite::memory:").await.unwrap();
        crate::rfqs::insert(
            &pool,
            &studio_types::Rfq {
                id: "rfq-1".into(),
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
        pool
    }

    #[tokio::test]
    async fn round_trips_and_is_a_singleton_per_rfq() {
        let pool = seeded_pool().await;
        let workroom = Workroom {
            rfq_id: "rfq-1".into(),
            channel_id: "0b5b7a86-6a45-4f7f-9207-3e069b7f0b0e".into(),
            create_event_id: "57fc8b6149f1c5d3ba5f3e801fc2219f92159311062c5876a4403d24ff98c431"
                .into(),
            created_at: ts("2026-08-01T17:00:00Z"),
        };
        record(&pool, &workroom).await.unwrap();

        let back = get_by_rfq(&pool, "rfq-1").await.unwrap().unwrap();
        assert_eq!(back, workroom);
        assert!(get_by_rfq(&pool, "rfq-2").await.unwrap().is_none());

        let err = record(&pool, &workroom).await.unwrap_err();
        assert!(matches!(err, StoreError::Conflict(_)), "got {err:?}");
    }
}
