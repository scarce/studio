//! The public project view — assembly and naming. Pure functions: the
//! handler fetches the rows, this module decides what an outsider sees.

use chrono::{DateTime, Utc};
use studio_types::{
    Project, ProjectLinks, ProjectMilestone, ProjectQuote, ProjectState, ProjectWorkroom, Quote,
    QuoteStatus, Rfq,
};

/// `proj-<slug>-<shortid>`: slug from the demand query, short id for
/// uniqueness (channel names are not unique on the relay; the uuid is).
/// Lives here so the mirror (channel creation) and the project view (page)
/// can never disagree about a workroom's name.
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

/// Assemble the public view from the projection rows. The quote's status is
/// re-derived against `now` (fail-closed against sweep lag, same as the
/// quote read endpoint); everything commercial stays out by construction —
/// [`Project`] has no field that could carry an amount.
pub fn view(
    rfq: &Rfq,
    quote: Option<&Quote>,
    workroom: Option<(&str, DateTime<Utc>)>,
    links: ProjectLinks,
    now: DateTime<Utc>,
) -> Project {
    let quote = quote.map(|q| q.clone().at(now));
    let state = match &quote {
        None => ProjectState::RfqCaptured,
        Some(q) => match q.status {
            QuoteStatus::Quoted => ProjectState::Quoted,
            QuoteStatus::Lapsed => ProjectState::Lapsed,
            // ACCEPTED stands in for FUNDED while payments are stubbed
            // (PLAN.md §6); the workroom row is the WORKROOM_ACTIVE evidence.
            QuoteStatus::Accepted => match workroom {
                Some(_) => ProjectState::WorkroomActive,
                None => ProjectState::Funded,
            },
        },
    };

    Project {
        id: rfq.id.clone(),
        title: rfq.query.clone(),
        state,
        created_at: rfq.created_at,
        quote: quote.map(|q| ProjectQuote {
            milestones: q
                .milestones
                .iter()
                .map(|m| ProjectMilestone {
                    title: m.title.clone(),
                    description: m.description.clone(),
                })
                .collect(),
            timeline: q.timeline.clone(),
            expires_at: q.expires_at,
            accepted_at: q.accepted_at,
        }),
        workroom: workroom.map(|(channel_id, since)| ProjectWorkroom {
            name: workroom_name(rfq),
            channel_id: channel_id.to_string(),
            since,
        }),
        links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_types::{Amount, ChannelParams, MilestoneSpec, PayoutDestination, Split};

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn rfq() -> Rfq {
        Rfq {
            id: "3f6b2c1a-0000-4000-8000-000000000000".into(),
            query: "Solana priority fee forecast API".into(),
            product: None,
            monetization: None,
            competition: vec![],
            budget_ceiling: Some(Amount {
                amount: 900_000_000,
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            }),
            buyer_npub: "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy".into(),
            buyer_signature: None,
            created_at: ts("2026-08-01T14:00:00Z"),
        }
    }

    fn quote(status: QuoteStatus) -> Quote {
        Quote {
            rfq_id: rfq().id,
            id: "q-1".into(),
            price: Amount {
                amount: 250_000_000,
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            },
            milestones: vec![MilestoneSpec {
                title: "Forecast model".into(),
                description: "p50/p90 per program id".into(),
                amount: 250_000_000,
            }],
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
            gate_policy: studio_types::GatePolicy::studio_default(),
            policy_hash: "ab".repeat(32),
            expires_at: ts("2026-09-01T00:00:00Z"),
            status,
            created_at: ts("2026-08-01T15:00:00Z"),
            lapsed_at: None,
            accepted_at: (status == QuoteStatus::Accepted).then(|| ts("2026-08-01T16:00:00Z")),
        }
    }

    fn links() -> ProjectLinks {
        ProjectLinks {
            community_web: Some("https://scarce.communities.buzz.xyz".into()),
            buzz_desktop: "https://buzz.xyz".into(),
        }
    }

    const NOW: &str = "2026-08-02T00:00:00Z";

    #[test]
    fn state_ladder_matches_the_rows() {
        let rfq = rfq();
        let now = ts(NOW);
        let view_of = |q: Option<&Quote>, w| view(&rfq, q, w, links(), now);

        assert_eq!(view_of(None, None).state, ProjectState::RfqCaptured);
        assert_eq!(
            view_of(Some(&quote(QuoteStatus::Quoted)), None).state,
            ProjectState::Quoted
        );
        assert_eq!(
            view_of(Some(&quote(QuoteStatus::Accepted)), None).state,
            ProjectState::Funded
        );
        let project = view_of(
            Some(&quote(QuoteStatus::Accepted)),
            Some((
                "0b5b7a86-6a45-4f7f-9207-3e069b7f0b0e",
                ts("2026-08-01T17:00:00Z"),
            )),
        );
        assert_eq!(project.state, ProjectState::WorkroomActive);
        let workroom = project.workroom.unwrap();
        assert_eq!(
            workroom.name,
            "proj-solana-priority-fee-forecast-api-3f6b2c1a"
        );
        assert_eq!(workroom.channel_id, "0b5b7a86-6a45-4f7f-9207-3e069b7f0b0e");
    }

    #[test]
    fn expiry_is_rederived_against_now() {
        // Row still says QUOTED, but `now` is past expiry — the public view
        // must not advertise an acceptable quote (same rule as the API read).
        let expired = view(
            &rfq(),
            Some(&quote(QuoteStatus::Quoted)),
            None,
            links(),
            ts("2026-09-02T00:00:00Z"),
        );
        assert_eq!(expired.state, ProjectState::Lapsed);
    }

    #[test]
    fn no_commercial_field_survives_serialization() {
        // The quote carries price, per-milestone amounts, payout splits and
        // the rfq a budget ceiling; the public JSON must contain none of it.
        let project = view(
            &rfq(),
            Some(&quote(QuoteStatus::Accepted)),
            Some((
                "0b5b7a86-6a45-4f7f-9207-3e069b7f0b0e",
                ts("2026-08-01T17:00:00Z"),
            )),
            links(),
            ts(NOW),
        );
        let json = serde_json::to_string(&project).unwrap();
        for leak in [
            "price",
            "amount",
            "250000000",
            "900000000",
            "payout",
            "splits",
            "bps",
            "CrewAgentA",
            "budget",
            "gate_policy",
            "policy_hash",
            "npub",
        ] {
            assert!(
                !json.contains(leak),
                "public project JSON leaks `{leak}`: {json}"
            );
        }
    }
}
