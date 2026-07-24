use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    ClaimInput, ClaimKind, MemoryDb, MemoryProcessingState, MemoryTier, PersonId, RememberInput,
    Result, SearchInput, SourceKind, TenantId,
};

/// Feature flag used for all personality-gated memory items.
pub const FEATURE_FLAG: &str = "personality";

// ---------------------------------------------------------------------------
// Turn-taking
// ---------------------------------------------------------------------------

/// The router's decision for a single conversational event.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnAction {
    Speak,
    StaySilent,
    React,
    ContinuePending,
}

/// A turn-taking decision recorded as evidence memory.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TurnDecision {
    pub epoch: u64,
    pub action: TurnAction,
    pub strategy: String,
    pub addressee: Option<String>,
    pub confidence_basis_points: u16,
    pub rationale: String,
}

/// A normalized conversation event from the platform.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConversationEvent {
    pub epoch: u64,
    pub participant: String,
    pub event_kind: String,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Social signals
// ---------------------------------------------------------------------------

/// A derived social signal from conversation events.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SocialSignal {
    pub signal_kind: String,
    pub participant: String,
    pub value: String,
    pub epoch: u64,
}

// ---------------------------------------------------------------------------
// Social norms (voice card)
// ---------------------------------------------------------------------------

/// A versioned, evidence-cited voice card for a conversation scope.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VoiceCard {
    pub scope: String,
    pub version: u64,
    pub register: String,
    pub humor: String,
    pub lexicon: Vec<String>,
    pub banned_phrases: Vec<String>,
    pub roles: Vec<String>,
    pub taboos: Vec<String>,
    pub in_jokes: Vec<String>,
    pub group_norms: Vec<String>,
    pub confidence_basis_points: u16,
}

// ---------------------------------------------------------------------------
// Theory of mind
// ---------------------------------------------------------------------------

/// An expiring hypothesis about a participant's mental state.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MindHypothesis {
    pub participant: String,
    pub belief: String,
    pub emotion: Option<String>,
    pub goal: Option<String>,
    pub predicted_reaction: Option<String>,
    pub confidence_basis_points: u16,
    pub valid_until: Option<crate::Timestamp>,
}

// ---------------------------------------------------------------------------
// Personas
// ---------------------------------------------------------------------------

/// A typed persona blueprint for a fictional agent or test population.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PersonaBlueprint {
    pub name: String,
    pub traits: Vec<String>,
    pub system_prompt: String,
    pub constraints: Vec<String>,
    pub citations: Vec<String>,
}

// ---------------------------------------------------------------------------
// Social observability
// ---------------------------------------------------------------------------

/// An evidence-cited finding from post-conversation analysis.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ObservationFinding {
    pub scope: String,
    pub finding: String,
    pub evidence: Vec<String>,
    pub recommendation: Option<String>,
    pub severity: ObservationSeverity,
}

/// Severity of an observability finding.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSeverity {
    Info,
    Warning,
    Critical,
}

// ---------------------------------------------------------------------------
// Personality runtime — backed by zkr MemoryDb
// ---------------------------------------------------------------------------

/// A behavioral runtime that stores all personality data as feature-gated
/// evidence memory in `zkr`.
///
/// Every memory item is written with `feature_flag: "personality"` so it is
/// hidden from default searches and only retrieved when the caller passes
/// `enabled_features: ["personality"]`.
pub struct Personality {
    db: MemoryDb,
    tenant_id: TenantId,
    person_id: PersonId,
}

impl Personality {
    pub fn new(db: MemoryDb, tenant_id: TenantId, person_id: PersonId) -> Self {
        Self {
            db,
            tenant_id,
            person_id,
        }
    }

    // --- Turn-taking -------------------------------------------------------

    /// Record a turn-taking decision as evidence memory.
    pub fn record_turn_decision(&mut self, decision: &TurnDecision) -> Result<()> {
        let now = now_seconds();
        let text = format!(
            "Turn epoch {}: {:?} via \"{}\" (confidence {}bps) — {}",
            decision.epoch,
            decision.action,
            decision.strategy,
            decision.confidence_basis_points,
            decision.rationale
        );
        let claim = ClaimInput {
            subject: format!("turn:{}", decision.epoch),
            predicate: "action".to_string(),
            value: format!(
                "{:?} via {} — {}",
                decision.action, decision.strategy, decision.rationale
            ),
            kind: ClaimKind::Task,
            valid_from: now,
            tier: MemoryTier::ShortTerm,
            processing_state: MemoryProcessingState::Processed,
        };
        self.db.remember(RememberInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            ingestion_key: Some(format!("turn:{}", decision.epoch)),
            kind: SourceKind::Conversation,
            text,
            captured_at: now,
            recorded_at: now,
            claim: Some(claim),
            feature_flag: Some(FEATURE_FLAG.into()),
        })?;
        Ok(())
    }

    /// Record a raw conversation event for signal derivation.
    pub fn record_event(&mut self, event: &ConversationEvent) -> Result<()> {
        let now = now_seconds();
        let text = format!(
            "[epoch {}] {} {}: {}",
            event.epoch, event.event_kind, event.participant, event.content
        );
        self.db.remember(RememberInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            ingestion_key: Some(format!("event:{}:{}", event.epoch, nanos())),
            kind: SourceKind::Conversation,
            text,
            captured_at: now,
            recorded_at: now,
            claim: None,
            feature_flag: Some(FEATURE_FLAG.into()),
        })?;
        Ok(())
    }

    /// Retrieve recent turn context for the agent.
    pub fn turn_context(&self, query: &str, limit: u32) -> Result<Vec<String>> {
        self.search_personality(query, limit)
    }

    // --- Social signals ----------------------------------------------------

    /// Record a derived social signal.
    pub fn record_signal(&mut self, signal: &SocialSignal) -> Result<()> {
        let now = now_seconds();
        let text = format!(
            "Signal {} for {} at epoch {}: {}",
            signal.signal_kind, signal.participant, signal.epoch, signal.value
        );
        self.db.remember(RememberInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            ingestion_key: Some(format!(
                "signal:{}:{}:{}",
                signal.signal_kind, signal.participant, signal.epoch
            )),
            kind: SourceKind::Conversation,
            text,
            captured_at: now,
            recorded_at: now,
            claim: None,
            feature_flag: Some(FEATURE_FLAG.into()),
        })?;
        Ok(())
    }

    // --- Social norms (voice card) -----------------------------------------

    /// Store or update a voice card for a conversation scope.
    pub fn store_voice_card(&mut self, card: &VoiceCard) -> Result<()> {
        let now = now_seconds();
        let text = format!(
            "Voice card v{} for \"{}\": register={}, humor={}, lexicon=[{}], banned=[{}], roles=[{}], taboos=[{}], in_jokes=[{}], norms=[{}] (confidence {}bps)",
            card.version,
            card.scope,
            card.register,
            card.humor,
            card.lexicon.join(", "),
            card.banned_phrases.join(", "),
            card.roles.join(", "),
            card.taboos.join(", "),
            card.in_jokes.join(", "),
            card.group_norms.join(", "),
            card.confidence_basis_points,
        );
        let claim = ClaimInput {
            subject: format!("norms:{}", card.scope),
            predicate: "voice_card".to_string(),
            value: format!(
                "v{} register={} humor={} norms={}",
                card.version,
                card.register,
                card.humor,
                card.group_norms.join(", ")
            ),
            kind: ClaimKind::ProfileFact,
            valid_from: now,
            tier: MemoryTier::LongTerm,
            processing_state: MemoryProcessingState::Processed,
        };
        self.db.remember(RememberInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            ingestion_key: Some(format!("voice_card:{}:{}", card.scope, card.version)),
            kind: SourceKind::Integration,
            text,
            captured_at: now,
            recorded_at: now,
            claim: Some(claim),
            feature_flag: Some(FEATURE_FLAG.into()),
        })?;
        Ok(())
    }

    /// Retrieve the voice card context for a scope.
    pub fn voice_card_context(&self, scope: &str, limit: u32) -> Result<Vec<String>> {
        self.search_personality(&format!("voice card norms {scope}"), limit)
    }

    // --- Theory of mind ----------------------------------------------------

    /// Record a theory-of-mind hypothesis about a participant.
    pub fn record_hypothesis(&mut self, hyp: &MindHypothesis) -> Result<()> {
        let now = now_seconds();
        let text = format!(
            "ToM for {}: belief=\"{}\", emotion={:?}, goal={:?}, predicted={:?}, confidence={}bps, valid_until={:?}",
            hyp.participant,
            hyp.belief,
            hyp.emotion,
            hyp.goal,
            hyp.predicted_reaction,
            hyp.confidence_basis_points,
            hyp.valid_until,
        );
        let claim = ClaimInput {
            subject: format!("tom:{}", hyp.participant),
            predicate: "hypothesis".to_string(),
            value: format!(
                "belief={} emotion={} goal={}",
                hyp.belief,
                hyp.emotion.as_deref().unwrap_or("unknown"),
                hyp.goal.as_deref().unwrap_or("unknown"),
            ),
            kind: ClaimKind::Recommendation,
            valid_from: now,
            tier: MemoryTier::ShortTerm,
            processing_state: MemoryProcessingState::Processed,
        };
        self.db.remember(RememberInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            ingestion_key: Some(format!("tom:{}:{}", hyp.participant, nanos())),
            kind: SourceKind::Conversation,
            text,
            captured_at: now,
            recorded_at: now,
            claim: Some(claim),
            feature_flag: Some(FEATURE_FLAG.into()),
        })?;
        Ok(())
    }

    /// Retrieve theory-of-mind context for a participant.
    pub fn tom_context(&self, participant: &str, limit: u32) -> Result<Vec<String>> {
        self.search_personality(&format!("theory of mind {participant}"), limit)
    }

    // --- Personas ----------------------------------------------------------

    /// Store a persona blueprint.
    pub fn store_persona(&mut self, persona: &PersonaBlueprint) -> Result<()> {
        let now = now_seconds();
        let text = format!(
            "Persona \"{}\": traits=[{}], constraints=[{}], prompt=\"{}\", citations=[{}]",
            persona.name,
            persona.traits.join(", "),
            persona.constraints.join(", "),
            persona.system_prompt,
            persona.citations.join(", "),
        );
        let claim = ClaimInput {
            subject: format!("persona:{}", persona.name),
            predicate: "blueprint".to_string(),
            value: format!(
                "traits={} prompt={}",
                persona.traits.join(", "),
                persona.system_prompt,
            ),
            kind: ClaimKind::ProfileFact,
            valid_from: now,
            tier: MemoryTier::LongTerm,
            processing_state: MemoryProcessingState::Processed,
        };
        self.db.remember(RememberInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            ingestion_key: Some(format!("persona:{}", persona.name)),
            kind: SourceKind::Document,
            text,
            captured_at: now,
            recorded_at: now,
            claim: Some(claim),
            feature_flag: Some(FEATURE_FLAG.into()),
        })?;
        Ok(())
    }

    /// Retrieve persona context by name or traits.
    pub fn persona_context(&self, query: &str, limit: u32) -> Result<Vec<String>> {
        self.search_personality(&format!("persona {query}"), limit)
    }

    // --- Social observability ---------------------------------------------

    /// Record an observability finding from post-conversation analysis.
    pub fn record_finding(&mut self, finding: &ObservationFinding) -> Result<()> {
        let now = now_seconds();
        let text = format!(
            "Finding [{:?}] for \"{}\": {} — evidence=[{}] recommendation={:?}",
            finding.severity,
            finding.scope,
            finding.finding,
            finding.evidence.join("; "),
            finding.recommendation,
        );
        let claim = ClaimInput {
            subject: format!("observation:{}", finding.scope),
            predicate: "finding".to_string(),
            value: format!(
                "{:?} {} — {}",
                finding.severity,
                finding.finding,
                finding.evidence.join("; "),
            ),
            kind: ClaimKind::Fact,
            valid_from: now,
            tier: MemoryTier::LongTerm,
            processing_state: MemoryProcessingState::Processed,
        };
        self.db.remember(RememberInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            ingestion_key: Some(format!("observation:{}:{}", finding.scope, nanos())),
            kind: SourceKind::Integration,
            text,
            captured_at: now,
            recorded_at: now,
            claim: Some(claim),
            feature_flag: Some(FEATURE_FLAG.into()),
        })?;
        Ok(())
    }

    /// Retrieve observability findings for a scope.
    pub fn observation_context(&self, scope: &str, limit: u32) -> Result<Vec<String>> {
        self.search_personality(&format!("observation finding {scope}"), limit)
    }

    // --- Prompt augmentation ----------------------------------------------

    /// Retrieve all personality context relevant to a query and format it
    /// as a system-prompt block.
    pub fn augment_prompt(&self, query: &str, base: &str) -> Result<String> {
        let mut context = Vec::new();
        context.extend(self.turn_context(query, 3)?);
        context.extend(self.voice_card_context(query, 2)?);
        context.extend(self.tom_context(query, 2)?);
        context.extend(self.persona_context(query, 2)?);
        context.extend(self.observation_context(query, 2)?);

        if context.is_empty() {
            return Ok(base.to_string());
        }

        let mut augmented = base.to_string();
        augmented.push_str("\n\n<personality_context>\n");
        for (i, item) in context.iter().enumerate() {
            augmented.push_str(&format!("{}. {item}\n", i + 1));
        }
        augmented.push_str("</personality_context>");
        Ok(augmented)
    }

    // --- Internal ----------------------------------------------------------

    fn search_personality(&self, query: &str, limit: u32) -> Result<Vec<String>> {
        let pack = self.db.search(SearchInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            query: query.to_string(),
            limit,
            query_embedding: None,
            as_of: None,
            enabled_features: vec![FEATURE_FLAG.into()],
        })?;
        Ok(pack.items.into_iter().map(|item| item.excerpt).collect())
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

    fn test_ids() -> (TenantId, PersonId) {
        (TenantId("t1".into()), PersonId("p1".into()))
    }

    #[test]
    fn records_and_retrieves_turn_decisions() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .record_turn_decision(&TurnDecision {
                epoch: 1,
                action: TurnAction::Speak,
                strategy: "direct_reply".into(),
                addressee: Some("user".into()),
                confidence_basis_points: 8500,
                rationale: "direct mention detected".into(),
            })
            .unwrap();

        let context = personality.turn_context("turn", 5).unwrap();
        assert_eq!(context.len(), 1);
        assert!(context[0].contains("direct_reply"));
    }

    #[test]
    fn voice_cards_are_stored_and_retrieved() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .store_voice_card(&VoiceCard {
                scope: "room-42".into(),
                version: 1,
                register: "casual".into(),
                humor: "dry".into(),
                lexicon: vec!["ship".into(), "deploy".into()],
                banned_phrases: vec![" ASAP".into()],
                roles: vec!["lead".into()],
                taboos: vec!["politics".into()],
                in_jokes: vec!["the incident".into()],
                group_norms: vec!["cite sources".into()],
                confidence_basis_points: 7200,
            })
            .unwrap();

        let context = personality.voice_card_context("room-42", 5).unwrap();
        assert_eq!(context.len(), 1);
        assert!(context[0].contains("casual"));
        assert!(context[0].contains("room-42") || context[0].contains("norms"));
    }

    #[test]
    fn theory_of_mind_hypotheses_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .record_hypothesis(&MindHypothesis {
                participant: "alice".into(),
                belief: "wants the deploy to succeed".into(),
                emotion: Some("anxious".into()),
                goal: Some("ship before deadline".into()),
                predicted_reaction: Some("relief if deploy works".into()),
                confidence_basis_points: 6000,
                valid_until: None,
            })
            .unwrap();

        let context = personality.tom_context("alice", 5).unwrap();
        assert_eq!(context.len(), 1);
        assert!(context[0].contains("anxious"));
    }

    #[test]
    fn personas_are_stored_and_retrieved() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .store_persona(&PersonaBlueprint {
                name: "helpful-engineer".into(),
                traits: vec!["patient".into(), "thorough".into()],
                system_prompt: "You are a helpful engineer.".into(),
                constraints: vec!["never delete prod".into()],
                citations: vec!["doc-1".into()],
            })
            .unwrap();

        let context = personality.persona_context("helpful", 5).unwrap();
        assert_eq!(context.len(), 1);
        assert!(context[0].contains("helpful-engineer"));
    }

    #[test]
    fn observations_are_stored_and_retrieved() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .record_finding(&ObservationFinding {
                scope: "thread-99".into(),
                finding: "agent interrupted user mid-sentence".into(),
                evidence: vec!["epoch 5".into(), "epoch 6".into()],
                recommendation: Some("increase silence threshold".into()),
                severity: ObservationSeverity::Warning,
            })
            .unwrap();

        let context = personality.observation_context("thread-99", 5).unwrap();
        assert_eq!(context.len(), 1);
        assert!(context[0].contains("interrupted"));
    }

    #[test]
    fn social_signals_are_stored() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .record_signal(&SocialSignal {
                signal_kind: "response_latency".into(),
                participant: "bob".into(),
                value: "3200ms".into(),
                epoch: 3,
            })
            .unwrap();

        let context = personality
            .search_personality("signal response latency", 5)
            .unwrap();
        assert_eq!(context.len(), 1);
        assert!(context[0].contains("3200ms"));
    }

    #[test]
    fn conversation_events_are_stored() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .record_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "hey can you help?".into(),
            })
            .unwrap();

        let context = personality.search_personality("hey help", 5).unwrap();
        assert_eq!(context.len(), 1);
        assert!(context[0].contains("hey can you help?"));
    }

    #[test]
    fn personality_items_are_hidden_without_feature_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .store_persona(&PersonaBlueprint {
                name: "secret".into(),
                traits: vec![],
                system_prompt: "hidden persona".into(),
                constraints: vec![],
                citations: vec![],
            })
            .unwrap();

        // Default search (no enabled_features) should not find personality items.
        let pack = personality
            .db
            .search(SearchInput {
                tenant_id: personality.tenant_id.clone(),
                person_id: personality.person_id.clone(),
                query: "secret hidden persona".into(),
                limit: 5,
                query_embedding: None,
                as_of: None,
                enabled_features: Vec::new(),
            })
            .unwrap();
        assert!(pack.items.is_empty());
    }

    #[test]
    fn augment_prompt_injects_context() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .store_voice_card(&VoiceCard {
                scope: "general".into(),
                version: 1,
                register: "professional".into(),
                humor: "subtle".into(),
                lexicon: vec![],
                banned_phrases: vec![],
                roles: vec![],
                taboos: vec![],
                in_jokes: vec![],
                group_norms: vec!["be concise".into()],
                confidence_basis_points: 8000,
            })
            .unwrap();

        let augmented = personality
            .augment_prompt("voice card norms general", "You are an agent.")
            .unwrap();
        assert!(augmented.contains("<personality_context>"));
        assert!(augmented.contains("professional"));
    }

    #[test]
    fn augment_prompt_returns_base_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let personality = Personality::new(db, tenant_id, person_id);

        let augmented = personality
            .augment_prompt("nothing", "Base prompt.")
            .unwrap();
        assert_eq!(augmented, "Base prompt.");
    }
}
