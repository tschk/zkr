use super::*;

/// Categorize a cited fact or task without introducing a second memory lifecycle.
#[derive(Debug, Deserialize)]
pub struct ProjectMemoryInput {
    pub project: String,
    pub category: String,
    #[serde(flatten)]
    pub memory: RememberInput,
}

#[derive(Debug, Deserialize)]
pub struct ProjectQuery {
    pub tenant_id: TenantId,
    pub person_id: PersonId,
    pub project: Option<String>,
    pub category: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Debug, Serialize)]
pub struct ProjectFact {
    pub project: String,
    pub category: String,
    pub claim_id: ClaimId,
    pub kind: ClaimKind,
    pub predicate: String,
    pub value: String,
    pub evidence_ids: Vec<EvidenceId>,
}

/// An AI or caller can name a record type and its fields without a schema migration.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectRecord {
    pub record_type: String,
    pub fields: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub connections: Vec<ProjectConnection>,
}

/// The target is an exact named subject, not an executable instruction or a foreign key.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConnection {
    pub relation: String,
    pub target: String,
}

#[derive(Debug, Deserialize)]
pub struct ProjectRecordInput {
    pub record: ProjectRecord,
    #[serde(flatten)]
    pub memory: ProjectMemoryInput,
}

fn validate_record(record: &ProjectRecord) -> Result<String> {
    let label = |name: &str, value: &str| -> Result<()> {
        require_text(name, value)?;
        if value.len() > 256 {
            return Err(Error::Invalid(format!("{name} exceeds 256 bytes")));
        }
        Ok(())
    };
    label("record_type", &record.record_type)?;
    if record.fields.len() > 64 || record.connections.len() > 64 {
        return Err(Error::Invalid(
            "record exceeds 64 fields or connections".into(),
        ));
    }
    for name in record.fields.keys() {
        label("field name", name)?;
    }
    for connection in &record.connections {
        label("relation", &connection.relation)?;
        label("target", &connection.target)?;
    }
    let value = serde_json::to_string(record)?;
    if value.len() > 16 * 1024 {
        return Err(Error::Invalid("record exceeds 16384 bytes".into()));
    }
    Ok(value)
}

impl MemoryDb {
    /// Store a caller-defined JSON record as the cited claim's complete value.
    /// Generic `correct` updates this serialized value; connections never grant authority.
    pub fn remember_project_record(&mut self, mut input: ProjectRecordInput) -> Result<Remembered> {
        let value = validate_record(&input.record)?;
        let claim = input
            .memory
            .memory
            .claim
            .as_mut()
            .ok_or_else(|| Error::Invalid("project record requires a cited claim".into()))?;
        claim.value = value;
        self.remember_project(input.memory)
    }

    /// Use a stable predicate per fact/task (for example `task:launch:status`).
    /// Update it through `correct`; deletion, replication and evidence stay ordinary ZKR operations.
    pub fn remember_project(&mut self, mut input: ProjectMemoryInput) -> Result<Remembered> {
        require_scope(&input.memory.tenant_id, &input.memory.person_id)?;
        require_text("project", &input.project)?;
        require_text("category", &input.category)?;
        let claim =
            input.memory.claim.as_mut().ok_or_else(|| {
                Error::Invalid("project memory requires a cited claim".to_owned())
            })?;
        // JSON preserves exact labels containing separators and quotes.
        claim.subject = format!(
            "zkr:project:{}",
            serde_json::to_string(&[&input.project, &input.category])?
        );
        self.remember(input.memory)
    }

    /// Current, processed project facts and tasks, optionally filtered by exact category/project.
    pub fn project_memories(&self, input: ProjectQuery) -> Result<Vec<ProjectFact>> {
        require_scope(&input.tenant_id, &input.person_id)?;
        if let Some(project) = &input.project {
            require_text("project", project)?;
        }
        if let Some(category) = &input.category {
            require_text("category", category)?;
        }
        let mut statement = self.connection.prepare(
            "SELECT c.subject,c.id,c.kind,c.predicate,c.value,min(e.id) FROM claims c
             JOIN claim_evidence ce ON ce.claim_id=c.id AND ce.tenant_id=c.tenant_id AND ce.person_id=c.person_id
             JOIN evidence e ON e.id=ce.evidence_id AND e.tenant_id=ce.tenant_id AND e.person_id=ce.person_id
             JOIN sources s ON s.id=e.source_id AND s.tenant_id=e.tenant_id AND s.person_id=e.person_id
             WHERE c.tenant_id=?1 AND c.person_id=?2 AND substr(c.subject,1,12)='zkr:project:'
             AND CASE WHEN json_valid(substr(c.subject,13)) THEN
               json_type(substr(c.subject,13))='array' AND json_array_length(substr(c.subject,13))=2
               AND json_type(substr(c.subject,13),'$[0]')='text' AND json_type(substr(c.subject,13),'$[1]')='text'
               AND (?3 IS NULL OR json_extract(substr(c.subject,13),'$[0]')=?3)
               AND (?4 IS NULL OR json_extract(substr(c.subject,13),'$[1]')=?4)
               ELSE 0 END
             AND c.status='accepted' AND c.valid_until IS NULL AND c.recorded_until IS NULL
             AND c.tier IN ('short_term','long_term') AND c.processing_state='processed'
             AND ce.relation='\"supports\"' AND e.deleted_at IS NULL AND s.deleted_at IS NULL
             AND s.feature_flag IS NULL
             GROUP BY c.id ORDER BY c.recorded_from DESC,c.id",
        )?;
        let rows = statement.query_map(
            params![
                input.tenant_id.0,
                input.person_id.0,
                input.project,
                input.category
            ],
            |row| {
                let subject: String = row.get(0)?;
                // SQLite accepts malformed Unicode escapes that Rust JSON correctly rejects.
                let Ok([project, category]) = serde_json::from_str::<[String; 2]>(&subject[12..])
                else {
                    return Ok(None);
                };
                if project.trim().is_empty() || category.trim().is_empty() {
                    return Ok(None);
                }
                let kind: String = row.get(2)?;
                Ok(Some(ProjectFact {
                    project,
                    category,
                    claim_id: ClaimId(row.get(1)?),
                    kind: serde_json::from_value(serde_json::Value::String(kind))
                        .map_err(sql_json_error)?,
                    predicate: row.get(3)?,
                    value: row.get(4)?,
                    evidence_ids: vec![EvidenceId(row.get(5)?)],
                }))
            },
        )?;
        collect_json_page(
            rows.filter_map(std::result::Result::transpose)
                .take(bounded_limit(input.limit) as usize),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn memory(project: &str, category: &str, person: &str, value: &str) -> ProjectMemoryInput {
        serde_json::from_value(json!({
            "project":project,"category":category,"tenant_id":"local","person_id":person,
            "kind":"conversation","text":format!("Launch status: {value}"),"captured_at":10,"recorded_at":10,
            "claim":{"subject":"ignored","predicate":"task:launch:status","value":value,"kind":"task","valid_from":10}
        })).unwrap()
    }

    fn query(person: &str, project: Option<&str>, category: Option<&str>) -> ProjectQuery {
        ProjectQuery {
            tenant_id: TenantId("local".into()),
            person_id: PersonId(person.into()),
            project: project.map(str::to_owned),
            category: category.map(str::to_owned),
            limit: 100,
        }
    }

    #[test]
    fn projects_and_categories_are_exact_scoped_and_cited() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        let remembered = db
            .remember_project(memory("Tofu", "work", "me", "planned"))
            .unwrap();
        db.remember_project(memory("Tofu", "personal", "me", "blocked"))
            .unwrap();
        db.remember_project(memory("Other", "work", "me", "active"))
            .unwrap();
        db.remember_project(memory("Tofu", "work", "someone", "private"))
            .unwrap();
        let facts = db
            .project_memories(query("me", Some("Tofu"), Some("work")))
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].value, "planned");
        assert_eq!(facts[0].evidence_ids, vec![remembered.evidence_id]);
        assert_eq!(facts[0].kind, ClaimKind::Task);
        assert_eq!(
            db.project_memories(query("me", Some("Tofu"), None))
                .unwrap()
                .len(),
            2
        );
        let mut other_tenant = query("me", None, None);
        other_tenant.tenant_id = TenantId("elsewhere".into());
        assert!(db.project_memories(other_tenant).unwrap().is_empty());
        assert_eq!(
            db.project_memories(query("me", None, Some("work")))
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn correction_and_source_deletion_preserve_project_lifecycle() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        let original = db
            .remember_project(memory("Tofu", "work", "me", "planned"))
            .unwrap();
        let corrected = db
            .correct(CorrectInput {
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                claim_id: original.claim_id.unwrap(),
                text: "Launch is completed".into(),
                value: "completed".into(),
                valid_at: 20,
                recorded_at: 20,
            })
            .unwrap();
        let facts = db
            .project_memories(query("me", Some("Tofu"), None))
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].value, "completed");
        assert_eq!(facts[0].claim_id, corrected.claim_id);
        db.delete_source(DeleteInput {
            tenant_id: TenantId("local".into()),
            person_id: PersonId("me".into()),
            source_id: corrected.source_id,
            deleted_at: 30,
        })
        .unwrap();
        assert!(
            db.project_memories(query("me", Some("Tofu"), None))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn project_memory_replicates_through_existing_export_apply() {
        let mut source = MemoryDb::open(":memory:").unwrap();
        source
            .remember_project(memory("Tofu", "work", "me", "planned"))
            .unwrap();
        let page = source
            .export(ExportInput {
                export_format: EXPORT_FORMAT_VERSION,
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                after_commit: 0,
                after_event_index: -1,
                high_water_mark: None,
                limit: 100,
            })
            .unwrap();
        assert!(page.complete);
        let mut replica = MemoryDb::open(":memory:").unwrap();
        replica
            .apply(ApplyInput {
                export_format: page.export_format,
                database_schema_version: Some(page.database_schema_version),
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                commits: page.commits,
            })
            .unwrap();
        let facts = replica
            .project_memories(query("me", Some("Tofu"), Some("work")))
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].value, "planned");
    }

    #[test]
    fn ai_defined_records_and_connections_keep_citations_and_lifecycle() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        let record: ProjectRecord = serde_json::from_value(json!({
            "record_type":"work_item",
            "fields":{"title":"Launch", "priority":2, "tags":["work"], "details":{"owner":"me"}},
            "connections":[{"relation":"blocked_by", "target":"cupboard:pairing"}]
        }))
        .unwrap();
        let remembered = db
            .remember_project_record(ProjectRecordInput {
                record: record.clone(),
                memory: memory("Tofu", "AI-created/launch", "me", "replaced"),
            })
            .unwrap();
        let facts = db
            .project_memories(query("me", Some("Tofu"), Some("AI-created/launch")))
            .unwrap();
        let stored: ProjectRecord = serde_json::from_str(&facts[0].value).unwrap();
        assert_eq!(stored, record);
        assert_eq!(facts[0].evidence_ids, vec![remembered.evidence_id]);
        let mut updated = stored;
        updated.fields.insert("priority".into(), json!(1));
        let corrected = db
            .correct(CorrectInput {
                tenant_id: TenantId("local".into()),
                person_id: PersonId("me".into()),
                claim_id: remembered.claim_id.unwrap(),
                text: "Launch is now priority one".into(),
                value: validate_record(&updated).unwrap(),
                valid_at: 20,
                recorded_at: 20,
            })
            .unwrap();
        let facts = db
            .project_memories(query("me", Some("Tofu"), None))
            .unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(
            serde_json::from_str::<ProjectRecord>(&facts[0].value).unwrap(),
            updated
        );
        assert!(
            db.project_memories(query("other", Some("Tofu"), None))
                .unwrap()
                .is_empty()
        );
        db.delete_source(DeleteInput {
            tenant_id: TenantId("local".into()),
            person_id: PersonId("me".into()),
            source_id: corrected.source_id,
            deleted_at: 30,
        })
        .unwrap();
        assert!(
            db.project_memories(query("me", Some("Tofu"), None))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn malformed_subjects_do_not_break_or_starve_valid_project_memories() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        db.remember_project(memory("Tofu", "work", "me", "visible"))
            .unwrap();
        for subject in [
            r#"zkr:project:["\ud800","work"]"#,
            r#"zkr:project:[" ",""]"#,
        ] {
            let mut input = memory("unused", "unused", "me", "malformed").memory;
            input.recorded_at = 20;
            input.claim.as_mut().unwrap().subject = subject.into();
            db.remember(input).unwrap();
        }
        let mut input = query("me", None, None);
        input.limit = 1;
        let facts = db.project_memories(input).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].project, "Tofu");
        assert_eq!(facts[0].value, "visible");
    }

    #[test]
    fn record_boundaries_reject_invalid_labels_and_oversized_data_before_writing() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        for record in [
            json!({"record_type":" ","fields":{}}),
            json!({"record_type":"meeting","fields":{"":"invalid"}}),
            json!({"record_type":"meeting","fields":{"notes":"x".repeat(16384)}}),
            json!({"record_type":"meeting","fields":{},"connections":[{"relation":" ","target":"someone"}]}),
            json!({"record_type":"meeting","fields":{},"connections":[{"relation":"with","target":""}]}),
            json!({"record_type":"meeting","fields":{},"connections":vec![json!({"relation":"with","target":"someone"});65]}),
        ] {
            let input = ProjectRecordInput {
                record: serde_json::from_value(record).unwrap(),
                memory: memory("Tofu", "work", "me", "ignored"),
            };
            assert!(db.remember_project_record(input).is_err());
            assert!(
                db.project_memories(query("me", None, None))
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn labels_do_not_collide_and_ineligible_memory_is_excluded() {
        let mut db = MemoryDb::open(":memory:").unwrap();
        db.remember_project(memory("a:b", "c", "me", "one"))
            .unwrap();
        db.remember_project(memory("a", "b:c", "me", "two"))
            .unwrap();
        for (tier, state, flag) in [
            ("archive", "processed", None),
            ("short_term", "pending", None),
            ("long_term", "processed", Some("private")),
        ] {
            let input = json!({
                "project":"hidden","category":"work","tenant_id":"local","person_id":"me",
                "kind":"conversation","text":"not visible","captured_at":10,"recorded_at":10,"feature_flag":flag,
                "claim":{"subject":"ignored","predicate":"status","value":"hidden","kind":"fact","valid_from":10,"tier":tier,"processing_state":state}
            });
            db.remember_project(serde_json::from_value(input).unwrap())
                .unwrap();
        }
        assert_eq!(
            db.project_memories(query("me", Some("a:b"), Some("c")))
                .unwrap()[0]
                .value,
            "one"
        );
        assert!(
            db.project_memories(query("me", Some("hidden"), None))
                .unwrap()
                .is_empty()
        );
        let mut missing = memory("Tofu", "work", "me", "none");
        missing.memory.claim = None;
        assert!(db.remember_project(missing).is_err());
        assert!(
            db.remember_project(memory(" ", "work", "me", "bad"))
                .is_err()
        );
    }
}
