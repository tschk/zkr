use super::*;
use crate::model::*;

#[test]
fn test_dream() {
    let mut db = MemoryDb {
        connection: rusqlite::Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let tenant = TenantId("t".into());
    let person = PersonId("p".into());

    let claim1_input = ClaimInput {
        subject: "zkr:project:[\"ProjectA\",\"FeatureX\"]".into(),
        predicate: "has_status".into(),
        value: "{\"record_type\": \"ProjectStatus\", \"categories\": [], \"fields\": {\"status\": \"in_progress\"}, \"connections\": [{\"relation\":\"blocks\",\"target\":\"FeatureZ\"}]}".into(),
        kind: ClaimKind::Fact,
        valid_from: 10,
        tier: MemoryTier::ShortTerm, // Keep this as short_term
        processing_state: MemoryProcessingState::Processed,
    };

    let remember_input1 = RememberInput {
        tenant_id: tenant.clone(),
        person_id: person.clone(),
        ingestion_key: None,
        kind: SourceKind::Conversation,
        text: "ProjectA FeatureX is in progress".into(),
        captured_at: 10,
        recorded_at: 10,
        claim: Some(claim1_input),
        feature_flag: None,
        aliases: vec![],
    };
    db.remember(remember_input1).unwrap();

    let remember_input2 = RememberInput {
        tenant_id: tenant.clone(),
        person_id: person.clone(),
        ingestion_key: None,
        kind: SourceKind::Conversation,
        text: "ProjectA FeatureY is blocked".into(),
        captured_at: 10,
        recorded_at: 10,
        claim: Some(ClaimInput {
            subject: "zkr:project:[\"ProjectA\",\"FeatureY\"]".into(),
            predicate: "has_status".into(),
            value: "{\"record_type\": \"ProjectStatus\", \"categories\": [], \"fields\": {\"status\": \"blocked\"}, \"connections\": [{\"relation\":\"blocked_by\",\"target\":\"FeatureX\"}]}".into(),
            kind: ClaimKind::Fact,
            valid_from: 10,
            tier: MemoryTier::ShortTerm,
            processing_state: MemoryProcessingState::Processed,
        }),
        feature_flag: None,
        aliases: vec![],
    };
    db.remember(remember_input2).unwrap();

    let res = db
        .dream(DreamInput {
            tenant_id: tenant.clone(),
            person_id: person.clone(),
            model: None,
            version: None,
            min_similarity_basis_points: 0,
            limit: 10,
        })
        .unwrap();

    assert_eq!(res.short_term_claims.len(), 2);

    assert!(!res.categories.is_empty());
    assert_eq!(res.categories[0].project, "ProjectA");

    assert!(!res.schemas.is_empty());
    assert_eq!(res.schemas[0].record_type, "ProjectStatus");
    assert_eq!(res.schemas[0].record_count, 2);

    // Check unresolved targets
    assert!(!res.unresolved_targets.is_empty());

    // Check hubs
    assert!(!res.hubs.is_empty());
}

#[test]
fn test_dream_invalid_embedding() {
    let mut db = MemoryDb {
        connection: rusqlite::Connection::open_in_memory().unwrap(),
    };
    db.migrate().unwrap();

    let tenant = TenantId("t".into());
    let person = PersonId("p".into());

    let res = db.dream(DreamInput {
        tenant_id: tenant.clone(),
        person_id: person.clone(),
        model: Some("model".into()),
        version: None, // Missing version
        min_similarity_basis_points: 0,
        limit: 10,
    });
    assert!(res.is_err());
}
