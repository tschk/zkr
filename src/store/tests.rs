use super::*;
use std::collections::HashSet;

fn remember(tenant: &str, person: &str, value: &str) -> RememberInput {
    RememberInput {
        tenant_id: TenantId(tenant.into()),
        person_id: PersonId(person.into()),
        ingestion_key: None,
        kind: SourceKind::Conversation,
        text: format!("Sam works at {value}"),
        captured_at: 10,
        recorded_at: 10,
        claim: Some(ClaimInput {
            subject: "Sam".into(),
            predicate: "employer".into(),
            value: value.into(),
            kind: ClaimKind::Fact,
            valid_from: 10,
        }),
    }
}

fn remember_raw(tenant: &str, person: &str, text: &str) -> RememberInput {
    RememberInput {
        tenant_id: TenantId(tenant.into()),
        person_id: PersonId(person.into()),
        ingestion_key: None,
        kind: SourceKind::Conversation,
        text: text.into(),
        captured_at: 10,
        recorded_at: 10,
        claim: None,
    }
}

fn hash_for(db: &MemoryDb, target: EmbeddingTarget) -> String {
    db.projection_input(&TenantId("a".into()), &PersonId("sam".into()), target)
        .unwrap()
        .input_hash
}

fn claim_with_older_last_support() -> (MemoryDb, SourceId, ClaimId) {
    let mut db = MemoryDb {
        connection: Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();
    let older = db
        .remember(remember_raw("a", "sam", "Earlier supporting evidence"))
        .unwrap();
    let mut claimed = remember("a", "sam", "Acme");
    claimed.captured_at = 20;
    claimed.recorded_at = 20;
    claimed.claim.as_mut().unwrap().valid_from = 20;
    let newer = db.remember(claimed).unwrap();
    let claim_id = newer.claim_id.unwrap();
    db.link_claim_evidence(ClaimEvidence {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        claim_id: claim_id.clone(),
        evidence_id: older.evidence_id,
        relation: EvidenceRelation::Supports,
        confidence_basis_points: 10_000,
    })
    .unwrap();
    db.delete_source(DeleteInput {
        tenant_id: TenantId("a".into()),
        person_id: PersonId("sam".into()),
        source_id: newer.source_id,
        deleted_at: 21,
    })
    .unwrap();
    (db, older.source_id, claim_id)
}

mod embeddings;
mod lifecycle;
mod migrations;
mod retrieval;
