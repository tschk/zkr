use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    ClaimInput, ClaimKind, MemoryDb, MemoryProcessingState, MemoryTier, PersonId, RememberInput,
    Remembered, Result, SearchInput, SourceKind, TenantId,
};

/// A self-improvement loop backed by `zkr` evidence memory.
///
/// Reflections are recorded as long-term skill claims. Before each agent run,
/// the most relevant lessons are retrieved and can be injected into a system
/// prompt.
pub struct SelfImprove {
    db: MemoryDb,
    tenant_id: TenantId,
    person_id: PersonId,
}

impl SelfImprove {
    pub fn new(db: MemoryDb, tenant_id: TenantId, person_id: PersonId) -> Self {
        Self {
            db,
            tenant_id,
            person_id,
        }
    }

    /// Record a reflection from a completed action.
    pub fn record(
        &mut self,
        context: &str,
        action: &str,
        outcome: &str,
        lesson: &str,
    ) -> Result<Remembered> {
        let now = now_seconds();
        let text =
            format!("Context: {context}\nAction: {action}\nOutcome: {outcome}\nLesson: {lesson}");
        let claim = ClaimInput {
            subject: context.to_string(),
            predicate: "improved".to_string(),
            value: lesson.to_string(),
            kind: ClaimKind::Skill,
            valid_from: now,
            tier: MemoryTier::LongTerm,
            processing_state: MemoryProcessingState::Processed,
        };
        self.db.remember(RememberInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            ingestion_key: Some(format!("self_improve:{}", nanos())),
            kind: SourceKind::Integration,
            text,
            captured_at: now,
            recorded_at: now,
            claim: Some(claim),
            feature_flag: None,
        })
    }

    /// Retrieve relevant lessons and augment the base prompt.
    pub fn augment(&self, query: &str, base: &str) -> Result<String> {
        let lessons = self.lessons(query, 5)?;
        if lessons.is_empty() {
            return Ok(base.to_string());
        }

        let mut augmented = base.to_string();
        augmented.push_str("\n\n<lessons_learned>\n");
        for (i, lesson) in lessons.iter().enumerate() {
            augmented.push_str(&format!("{}. {lesson}\n", i + 1));
        }
        augmented.push_str("</lessons_learned>");
        Ok(augmented)
    }

    /// Retrieve lesson excerpts relevant to `query`.
    pub fn lessons(&self, query: &str, limit: u32) -> Result<Vec<String>> {
        let mut seen = std::collections::HashSet::new();
        let mut lessons = Vec::new();

        for q in ["self improve lessons", query] {
            let pack = self.db.search(SearchInput {
                tenant_id: self.tenant_id.clone(),
                person_id: self.person_id.clone(),
                query: q.to_string(),
                limit,
                query_embedding: None,
                as_of: None,
                enabled_features: Vec::new(),
            })?;
            for item in pack.items {
                let text = item.excerpt.trim().to_string();
                if seen.insert(text.clone()) {
                    lessons.push(text);
                }
            }
        }

        lessons.truncate(limit as usize);
        Ok(lessons)
    }
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn test_ids() -> (TenantId, PersonId) {
        (TenantId("t1".into()), PersonId("p1".into()))
    }

    #[test]
    fn records_and_retrieves_lessons() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("memory.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut improve = SelfImprove::new(db, tenant_id, person_id);

        improve
            .record(
                "coding",
                "refactor loop",
                "success",
                "prefer iterator adapters",
            )
            .unwrap();

        let lessons = improve.lessons("refactor", 5).unwrap();
        assert_eq!(lessons.len(), 1);
        assert!(lessons[0].contains("prefer iterator adapters"));
    }

    #[test]
    fn augment_injects_lessons_into_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("memory.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut improve = SelfImprove::new(db, tenant_id, person_id);

        improve
            .record("testing", "add mocks", "success", "isolate IO in tests")
            .unwrap();

        let augmented = improve
            .augment("testing", "You are a helpful agent.")
            .unwrap();
        assert!(augmented.contains("<lessons_learned>"));
        assert!(augmented.contains("isolate IO in tests"));
    }

    #[test]
    fn deduplicates_lessons_across_queries() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("memory.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut improve = SelfImprove::new(db, tenant_id, person_id);

        improve
            .record("deploy", "use tags", "success", "pin image tags")
            .unwrap();

        let lessons = improve.lessons("deploy", 5).unwrap();
        let set: HashSet<_> = lessons.iter().cloned().collect();
        assert_eq!(lessons.len(), set.len());
    }
}
