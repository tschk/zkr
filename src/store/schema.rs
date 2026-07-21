use super::*;
use rusqlite::{Transaction, TransactionBehavior};

const SCHEMA_VERSION: i64 = 6;

pub(super) fn migrate(connection: &mut Connection) -> Result<()> {
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    let version = connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?;
    if !(0..=SCHEMA_VERSION).contains(&version) {
        return Err(Error::Invalid(format!(
            "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
        )));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if version < 1 {
        migrate_v1(&transaction)?;
    }
    repair_v1_shape(&transaction)?;
    if version < 2 {
        migrate_v2(&transaction)?;
    }
    if version < 3 {
        migrate_v3(&transaction)?;
    }
    if version < 4 {
        migrate_v4(&transaction)?;
    }
    if version < 5 {
        migrate_v5(&transaction)?;
    }
    if version < 6 {
        migrate_v6(&transaction)?;
    }
    set_version(&transaction, SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

fn migrate_v1(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sources(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, revision INTEGER NOT NULL, kind TEXT NOT NULL, content TEXT NOT NULL, captured_at INTEGER NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
        CREATE INDEX IF NOT EXISTS sources_scope ON sources(tenant_id, person_id, id);
        CREATE VIRTUAL TABLE IF NOT EXISTS source_fts USING fts5(source_id UNINDEXED, tenant_id UNINDEXED, person_id UNINDEXED, content, tokenize='unicode61');
        CREATE TABLE IF NOT EXISTS evidence(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, source_id TEXT NOT NULL REFERENCES sources(id), source_revision INTEGER NOT NULL, quote TEXT NOT NULL, recorded_at INTEGER NOT NULL, deleted_at INTEGER);
        CREATE INDEX IF NOT EXISTS evidence_scope ON evidence(tenant_id, person_id, source_id);
        CREATE TABLE IF NOT EXISTS claims(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, subject TEXT NOT NULL, predicate TEXT NOT NULL, value TEXT NOT NULL, valid_from INTEGER NOT NULL, valid_until INTEGER, recorded_from INTEGER NOT NULL, recorded_until INTEGER, status TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS claims_scope ON claims(tenant_id, person_id, status);
        CREATE TABLE IF NOT EXISTS claim_evidence(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES claims(id), evidence_id TEXT NOT NULL REFERENCES evidence(id), relation TEXT NOT NULL, confidence_basis_points INTEGER NOT NULL, PRIMARY KEY(tenant_id, person_id, claim_id, evidence_id));
        CREATE TABLE IF NOT EXISTS daily_reviews(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, day TEXT NOT NULL, summary TEXT NOT NULL, evidence_ids TEXT NOT NULL, recorded_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS reviews_scope ON daily_reviews(tenant_id, person_id, day);
        CREATE TABLE IF NOT EXISTS embeddings(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT NOT NULL, model TEXT NOT NULL, version TEXT NOT NULL, dimension INTEGER NOT NULL, input_hash TEXT NOT NULL, normalization TEXT NOT NULL, distance TEXT NOT NULL, vector TEXT NOT NULL, PRIMARY KEY(tenant_id, person_id, target_kind, target_id, model, version));
        CREATE INDEX IF NOT EXISTS embeddings_scope ON embeddings(tenant_id, person_id, target_kind, target_id);
        ",
    )?;
    Ok(())
}

fn repair_v1_shape(transaction: &Transaction<'_>) -> Result<()> {
    ensure_column(transaction, "sources", "ingestion_key", "TEXT")?;
    ensure_column(
        transaction,
        "embeddings",
        "target_revision",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        transaction,
        "embeddings",
        "created_at",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    transaction.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS sources_ingestion_key ON sources(tenant_id, person_id, ingestion_key) WHERE ingestion_key IS NOT NULL;",
    )?;
    Ok(())
}

fn migrate_v2(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS evidence_locators(tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, evidence_id TEXT NOT NULL REFERENCES evidence(id), device_id TEXT NOT NULL, provider TEXT NOT NULL, stream_id TEXT NOT NULL, segment_id TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, PRIMARY KEY(tenant_id, person_id, evidence_id));
        ",
    )?;
    Ok(())
}

fn migrate_v3(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS sources_scope ON sources(tenant_id, person_id, id);
        CREATE INDEX IF NOT EXISTS evidence_scope ON evidence(tenant_id, person_id, source_id);
        CREATE INDEX IF NOT EXISTS claims_scope ON claims(tenant_id, person_id, status);
        CREATE INDEX IF NOT EXISTS reviews_scope ON daily_reviews(tenant_id, person_id, day);
        CREATE INDEX IF NOT EXISTS embeddings_scope ON embeddings(tenant_id, person_id, target_kind, target_id);
        ",
    )?;
    Ok(())
}

fn migrate_v4(transaction: &Transaction<'_>) -> Result<()> {
    validate_scope(transaction, false, false)?;
    install_core_scope_triggers(transaction)?;
    Ok(())
}

fn migrate_v5(transaction: &Transaction<'_>) -> Result<()> {
    ensure_column(
        transaction,
        "claims",
        "kind",
        "TEXT NOT NULL DEFAULT 'fact'",
    )?;
    transaction.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS profile_entries(id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, person_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, stability TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES claims(id), recorded_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS profile_entries_scope ON profile_entries(tenant_id, person_id, recorded_at);
        ",
    )?;
    install_profile_triggers(transaction)?;
    Ok(())
}

fn migrate_v6(transaction: &Transaction<'_>) -> Result<()> {
    ensure_column(transaction, "sources", "origin_evidence_id", "TEXT")?;
    ensure_column(transaction, "sources", "origin_claim_id", "TEXT")?;
    transaction.execute(
        "UPDATE sources SET origin_evidence_id = (SELECT MIN(e.id) FROM evidence e WHERE e.source_id = sources.id AND e.tenant_id = sources.tenant_id AND e.person_id = sources.person_id) WHERE origin_evidence_id IS NULL AND (SELECT COUNT(*) FROM evidence e WHERE e.source_id = sources.id AND e.tenant_id = sources.tenant_id AND e.person_id = sources.person_id) = 1",
        [],
    )?;
    transaction.execute(
        r#"UPDATE sources SET origin_claim_id = (SELECT MIN(ce.claim_id) FROM claim_evidence ce WHERE ce.evidence_id = sources.origin_evidence_id AND ce.tenant_id = sources.tenant_id AND ce.person_id = sources.person_id AND ce.relation = '"supports"') WHERE origin_claim_id IS NULL AND origin_evidence_id IS NOT NULL AND (SELECT COUNT(*) FROM claim_evidence ce WHERE ce.evidence_id = sources.origin_evidence_id AND ce.tenant_id = sources.tenant_id AND ce.person_id = sources.person_id AND ce.relation = '"supports"') = 1"#,
        [],
    )?;
    validate_scope(transaction, false, true)?;
    transaction.execute_batch(
        "DROP TRIGGER IF EXISTS profile_entry_scope_insert;
         DROP TRIGGER IF EXISTS profile_entry_scope_update;",
    )?;
    validate_profile_references(transaction)?;
    transaction.execute(
        "UPDATE profile_entries SET key = (SELECT c.predicate FROM claims c WHERE c.id = profile_entries.claim_id AND c.tenant_id = profile_entries.tenant_id AND c.person_id = profile_entries.person_id AND c.kind = 'profile_fact'), value = (SELECT c.value FROM claims c WHERE c.id = profile_entries.claim_id AND c.tenant_id = profile_entries.tenant_id AND c.person_id = profile_entries.person_id AND c.kind = 'profile_fact') WHERE EXISTS (SELECT 1 FROM claims c WHERE c.id = profile_entries.claim_id AND c.tenant_id = profile_entries.tenant_id AND c.person_id = profile_entries.person_id AND c.kind = 'profile_fact')",
        [],
    )?;
    transaction.execute(
        "DELETE FROM profile_entries AS old WHERE EXISTS (SELECT 1 FROM profile_entries newer WHERE newer.tenant_id = old.tenant_id AND newer.person_id = old.person_id AND newer.key = old.key AND (newer.recorded_at > old.recorded_at OR (newer.recorded_at = old.recorded_at AND newer.id > old.id)))",
        [],
    )?;
    validate_scope(transaction, true, true)?;
    transaction.execute_batch(
        "
        CREATE UNIQUE INDEX IF NOT EXISTS profile_entries_current ON profile_entries(tenant_id, person_id, key);
        CREATE TRIGGER IF NOT EXISTS source_origin_scope_insert BEFORE INSERT ON sources FOR EACH ROW WHEN (NEW.origin_evidence_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM evidence WHERE id = NEW.origin_evidence_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) OR (NEW.origin_claim_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM claims WHERE id = NEW.origin_claim_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) BEGIN SELECT RAISE(ABORT, 'source origin scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS source_origin_scope_update BEFORE UPDATE OF origin_evidence_id, origin_claim_id, tenant_id, person_id ON sources FOR EACH ROW WHEN (NEW.origin_evidence_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM evidence WHERE id = NEW.origin_evidence_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) OR (NEW.origin_claim_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM claims WHERE id = NEW.origin_claim_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) BEGIN SELECT RAISE(ABORT, 'source origin scope mismatch'); END;
        ",
    )?;
    install_profile_triggers(transaction)?;
    Ok(())
}

fn ensure_column(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let exists = transaction.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)"),
        [column],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        transaction.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

fn set_version(transaction: &Transaction<'_>, version: i64) -> Result<()> {
    transaction.execute_batch(&format!("PRAGMA user_version = {version};"))?;
    Ok(())
}

fn validate_scope(
    transaction: &Transaction<'_>,
    require_profile_content: bool,
    require_origins: bool,
) -> Result<()> {
    let mut checks = vec![
        (
            "evidence",
            "SELECT EXISTS(SELECT 1 FROM evidence e LEFT JOIN sources s ON s.id = e.source_id AND s.tenant_id = e.tenant_id AND s.person_id = e.person_id WHERE s.id IS NULL)",
        ),
        (
            "evidence locator",
            "SELECT EXISTS(SELECT 1 FROM evidence_locators l LEFT JOIN evidence e ON e.id = l.evidence_id AND e.tenant_id = l.tenant_id AND e.person_id = l.person_id WHERE e.id IS NULL)",
        ),
        (
            "claim evidence",
            "SELECT EXISTS(SELECT 1 FROM claim_evidence ce LEFT JOIN claims c ON c.id = ce.claim_id AND c.tenant_id = ce.tenant_id AND c.person_id = ce.person_id LEFT JOIN evidence e ON e.id = ce.evidence_id AND e.tenant_id = ce.tenant_id AND e.person_id = ce.person_id WHERE c.id IS NULL OR e.id IS NULL)",
        ),
        (
            "embedding",
            "SELECT EXISTS(SELECT 1 FROM embeddings p WHERE p.target_kind NOT IN ('source', 'evidence', 'claim') OR (p.target_kind = 'source' AND NOT EXISTS (SELECT 1 FROM sources s WHERE s.id = p.target_id AND s.tenant_id = p.tenant_id AND s.person_id = p.person_id)) OR (p.target_kind = 'evidence' AND NOT EXISTS (SELECT 1 FROM evidence e WHERE e.id = p.target_id AND e.tenant_id = p.tenant_id AND e.person_id = p.person_id)) OR (p.target_kind = 'claim' AND NOT EXISTS (SELECT 1 FROM claims c WHERE c.id = p.target_id AND c.tenant_id = p.tenant_id AND c.person_id = p.person_id)))",
        ),
        (
            "daily review",
            "SELECT EXISTS(SELECT 1 FROM daily_reviews r LEFT JOIN json_each(CASE WHEN json_valid(r.evidence_ids) THEN r.evidence_ids ELSE '[]' END) citation LEFT JOIN evidence e ON e.id = citation.value AND e.tenant_id = r.tenant_id AND e.person_id = r.person_id WHERE json_valid(r.evidence_ids) = 0 OR e.id IS NULL)",
        ),
    ];
    if require_origins {
        checks.push((
            "source origin",
            "SELECT EXISTS(SELECT 1 FROM sources s LEFT JOIN evidence e ON e.id = s.origin_evidence_id AND e.tenant_id = s.tenant_id AND e.person_id = s.person_id LEFT JOIN claims c ON c.id = s.origin_claim_id AND c.tenant_id = s.tenant_id AND c.person_id = s.person_id WHERE (s.origin_evidence_id IS NOT NULL AND e.id IS NULL) OR (s.origin_claim_id IS NOT NULL AND c.id IS NULL))",
        ));
    }
    if require_profile_content {
        checks.push((
            "profile entry",
            "SELECT EXISTS(SELECT 1 FROM profile_entries p LEFT JOIN claims c ON c.id = p.claim_id AND c.tenant_id = p.tenant_id AND c.person_id = p.person_id WHERE c.id IS NULL OR c.kind != 'profile_fact' OR c.predicate != p.key OR c.value != p.value)",
        ));
    }
    for (name, query) in checks {
        if transaction.query_row(query, [], |row| row.get::<_, bool>(0))? {
            return Err(Error::Invalid(format!(
                "legacy {name} is inconsistent with schema invariants"
            )));
        }
    }
    Ok(())
}

fn validate_profile_references(transaction: &Transaction<'_>) -> Result<()> {
    let inconsistent = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM profile_entries p LEFT JOIN claims c ON c.id = p.claim_id AND c.tenant_id = p.tenant_id AND c.person_id = p.person_id WHERE c.id IS NULL OR c.kind != 'profile_fact')",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if inconsistent {
        return Err(Error::Invalid(
            "legacy profile entry is inconsistent with schema invariants".to_owned(),
        ));
    }
    Ok(())
}

fn install_core_scope_triggers(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "
        CREATE TRIGGER IF NOT EXISTS evidence_scope_insert BEFORE INSERT ON evidence FOR EACH ROW WHEN NOT EXISTS (SELECT 1 FROM sources WHERE id = NEW.source_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id) BEGIN SELECT RAISE(ABORT, 'evidence source scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS evidence_scope_update BEFORE UPDATE OF source_id, tenant_id, person_id ON evidence FOR EACH ROW WHEN NOT EXISTS (SELECT 1 FROM sources WHERE id = NEW.source_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id) BEGIN SELECT RAISE(ABORT, 'evidence source scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS locator_scope_insert BEFORE INSERT ON evidence_locators FOR EACH ROW WHEN NOT EXISTS (SELECT 1 FROM evidence WHERE id = NEW.evidence_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id) BEGIN SELECT RAISE(ABORT, 'evidence locator scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS locator_scope_update BEFORE UPDATE OF evidence_id, tenant_id, person_id ON evidence_locators FOR EACH ROW WHEN NOT EXISTS (SELECT 1 FROM evidence WHERE id = NEW.evidence_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id) BEGIN SELECT RAISE(ABORT, 'evidence locator scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS claim_evidence_scope_insert BEFORE INSERT ON claim_evidence FOR EACH ROW WHEN NOT EXISTS (SELECT 1 FROM claims WHERE id = NEW.claim_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id) OR NOT EXISTS (SELECT 1 FROM evidence WHERE id = NEW.evidence_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id) BEGIN SELECT RAISE(ABORT, 'claim evidence scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS claim_evidence_scope_update BEFORE UPDATE OF claim_id, evidence_id, tenant_id, person_id ON claim_evidence FOR EACH ROW WHEN NOT EXISTS (SELECT 1 FROM claims WHERE id = NEW.claim_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id) OR NOT EXISTS (SELECT 1 FROM evidence WHERE id = NEW.evidence_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id) BEGIN SELECT RAISE(ABORT, 'claim evidence scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS embedding_scope_insert BEFORE INSERT ON embeddings FOR EACH ROW WHEN NEW.target_kind NOT IN ('source', 'evidence', 'claim') OR (NEW.target_kind = 'source' AND NOT EXISTS (SELECT 1 FROM sources WHERE id = NEW.target_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) OR (NEW.target_kind = 'evidence' AND NOT EXISTS (SELECT 1 FROM evidence WHERE id = NEW.target_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) OR (NEW.target_kind = 'claim' AND NOT EXISTS (SELECT 1 FROM claims WHERE id = NEW.target_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) BEGIN SELECT RAISE(ABORT, 'embedding target scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS embedding_scope_update BEFORE UPDATE OF target_kind, target_id, tenant_id, person_id ON embeddings FOR EACH ROW WHEN NEW.target_kind NOT IN ('source', 'evidence', 'claim') OR (NEW.target_kind = 'source' AND NOT EXISTS (SELECT 1 FROM sources WHERE id = NEW.target_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) OR (NEW.target_kind = 'evidence' AND NOT EXISTS (SELECT 1 FROM evidence WHERE id = NEW.target_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) OR (NEW.target_kind = 'claim' AND NOT EXISTS (SELECT 1 FROM claims WHERE id = NEW.target_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id)) BEGIN SELECT RAISE(ABORT, 'embedding target scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS review_scope_insert BEFORE INSERT ON daily_reviews FOR EACH ROW WHEN json_valid(NEW.evidence_ids) = 0 OR EXISTS (SELECT 1 FROM json_each(NEW.evidence_ids) citation LEFT JOIN evidence e ON e.id = citation.value AND e.tenant_id = NEW.tenant_id AND e.person_id = NEW.person_id WHERE e.id IS NULL) BEGIN SELECT RAISE(ABORT, 'daily review evidence scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS review_scope_update BEFORE UPDATE OF tenant_id, person_id, evidence_ids ON daily_reviews FOR EACH ROW WHEN json_valid(NEW.evidence_ids) = 0 OR EXISTS (SELECT 1 FROM json_each(NEW.evidence_ids) citation LEFT JOIN evidence e ON e.id = citation.value AND e.tenant_id = NEW.tenant_id AND e.person_id = NEW.person_id WHERE e.id IS NULL) BEGIN SELECT RAISE(ABORT, 'daily review evidence scope mismatch'); END;
        ",
    )?;
    Ok(())
}

fn install_profile_triggers(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "
        CREATE TRIGGER IF NOT EXISTS profile_entry_scope_insert BEFORE INSERT ON profile_entries FOR EACH ROW WHEN NOT EXISTS (SELECT 1 FROM claims WHERE id = NEW.claim_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id AND kind = 'profile_fact' AND predicate = NEW.key AND value = NEW.value) BEGIN SELECT RAISE(ABORT, 'profile entry claim scope mismatch'); END;
        CREATE TRIGGER IF NOT EXISTS profile_entry_scope_update BEFORE UPDATE OF key, value, claim_id, tenant_id, person_id ON profile_entries FOR EACH ROW WHEN NOT EXISTS (SELECT 1 FROM claims WHERE id = NEW.claim_id AND tenant_id = NEW.tenant_id AND person_id = NEW.person_id AND kind = 'profile_fact' AND predicate = NEW.key AND value = NEW.value) BEGIN SELECT RAISE(ABORT, 'profile entry claim scope mismatch'); END;
        ",
    )?;
    Ok(())
}
