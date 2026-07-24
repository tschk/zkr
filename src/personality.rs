use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    ClaimInput, ClaimKind, MemoryDb, MemoryProcessingState, MemoryTier, PersonId, RememberInput,
    Result, SearchInput, SourceKind, TenantId, Timestamp,
};

/// Feature flag used for all personality-gated memory items.
pub const FEATURE_FLAG: &str = "personality";

// ---------------------------------------------------------------------------
// Turn-taking and proactivity
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

/// Hard rules the router evaluates before the learned policy.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RouterRules {
    /// If true, a direct mention forces Speak.
    pub mention_triggers_reply: bool,
    /// If true, a command (starting with / or !) forces Speak.
    pub command_triggers_reply: bool,
    /// Minimum epochs between agent replies to avoid spam.
    pub min_reply_interval: u64,
    /// Max consecutive agent turns before forcing silence.
    pub max_consecutive_turns: u64,
}

impl Default for RouterRules {
    fn default() -> Self {
        Self {
            mention_triggers_reply: true,
            command_triggers_reply: true,
            min_reply_interval: 0,
            max_consecutive_turns: 3,
        }
    }
}

/// The result of routing an event through hard rules + policy.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RouterResult {
    pub decision: TurnDecision,
    pub behavioral_context: Vec<String>,
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

/// Aggregated signal summary for a participant.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SignalSummary {
    pub participant: String,
    pub message_count: u64,
    pub avg_response_latency_ms: Option<f64>,
    pub participation_share: f64,
    pub conversation_velocity: f64,
    pub typing_without_send: u64,
    pub reaction_count: u64,
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
    pub supporting_event_epochs: Vec<u64>,
    pub valid_from: Timestamp,
    pub valid_until: Option<Timestamp>,
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
    pub valid_until: Option<Timestamp>,
}

/// A risk assessment for a candidate reply.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct RiskAssessment {
    pub misunderstanding_risk: u16,
    pub escalation_risk: u16,
    pub exclusion_risk: u16,
    pub churn_risk: u16,
    pub overall_risk_basis_points: u16,
    pub recommendation: RiskRecommendation,
}

/// What to do about a risk.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskRecommendation {
    #[default]
    Proceed,
    Refine,
    Abort,
}

/// A calibration record comparing a prediction to the real outcome.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CalibrationRecord {
    pub participant: String,
    pub predicted_reaction: String,
    pub actual_reaction: String,
    pub correct: bool,
    pub epoch: u64,
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

/// Validation result for a persona.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PersonaValidation {
    pub name: String,
    pub constraints_satisfied: bool,
    pub trait_count: usize,
    pub duplicate_traits: Vec<String>,
    pub empty_fields: Vec<String>,
    pub valid: bool,
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

/// A conversation health summary from observability analysis.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ConversationHealth {
    pub scope: String,
    pub total_turns: u64,
    pub agent_turns: u64,
    pub user_turns: u64,
    pub participation_balance: f64,
    pub error_rate: f64,
    pub findings: Vec<ObservationFinding>,
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
    epoch: u64,
    rules: RouterRules,
    pending_signals: HashMap<String, Vec<SocialSignal>>,
    recent_events: Vec<ConversationEvent>,
    consecutive_agent_turns: u64,
    last_agent_reply_epoch: Option<u64>,
}

impl Personality {
    pub fn new(db: MemoryDb, tenant_id: TenantId, person_id: PersonId) -> Self {
        Self {
            db,
            tenant_id,
            person_id,
            epoch: 0,
            rules: RouterRules::default(),
            pending_signals: HashMap::new(),
            recent_events: Vec::new(),
            consecutive_agent_turns: 0,
            last_agent_reply_epoch: None,
        }
    }

    pub fn db(&self) -> &MemoryDb {
        &self.db
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn person_id(&self) -> &PersonId {
        &self.person_id
    }

    pub fn with_rules(mut self, rules: RouterRules) -> Self {
        self.rules = rules;
        self
    }

    pub fn set_rules(&mut self, rules: RouterRules) {
        self.rules = rules;
    }

    /// Advance the epoch and return the new value.
    pub fn advance_epoch(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    pub fn current_epoch(&self) -> u64 {
        self.epoch
    }

    // --- Turn-taking: router ------------------------------------------------

    /// Evaluate an incoming event through hard rules + policy and return
    /// a routing decision with behavioral context.
    ///
    /// This is the core router from the plan: it checks hard rules first
    /// (mentions, commands, rate limits, consecutive turns), then applies
    /// a learned policy scoring appropriateness and timeliness.
    pub fn route_event(&mut self, event: &ConversationEvent) -> Result<RouterResult> {
        self.recent_events.push(event.clone());
        let epoch = event.epoch;

        // --- Hard rules (veto rules checked first) ---
        let mut action = TurnAction::StaySilent;
        let mut strategy = "observe".to_string();
        let mut confidence: u16 = 5000;
        let mut rationale = String::new();
        let mut hard_rule_hit = false;

        // Rate limit: too soon after last reply — veto all other rules.
        if self.rules.min_reply_interval > 0
            && let Some(last) = self.last_agent_reply_epoch
            && epoch - last < self.rules.min_reply_interval
        {
            action = TurnAction::StaySilent;
            strategy = "rate_limited".into();
            confidence = 8000;
            rationale = format!(
                "only {} epochs since last reply (min {})",
                epoch - last,
                self.rules.min_reply_interval
            );
            hard_rule_hit = true;
        }

        // Max consecutive turns: force silence — veto all other rules.
        if !hard_rule_hit && self.consecutive_agent_turns >= self.rules.max_consecutive_turns {
            action = TurnAction::StaySilent;
            strategy = "consecutive_limit".into();
            confidence = 8500;
            rationale = format!(
                "max consecutive turns ({}) reached",
                self.rules.max_consecutive_turns
            );
            hard_rule_hit = true;
        }

        // Direct mention forces Speak (if not vetoed).
        if !hard_rule_hit
            && self.rules.mention_triggers_reply
            && event.event_kind == "message"
            && event.content.contains("@agent")
        {
            action = TurnAction::Speak;
            strategy = "direct_reply".into();
            confidence = 9500;
            rationale = "direct mention detected".into();
            hard_rule_hit = true;
        }

        // Command forces Speak (if not vetoed).
        if !hard_rule_hit
            && self.rules.command_triggers_reply
            && event.event_kind == "message"
            && (event.content.starts_with('/') || event.content.starts_with('!'))
        {
            action = TurnAction::Speak;
            strategy = "command_response".into();
            confidence = 9000;
            rationale = "command detected".into();
            hard_rule_hit = true;
        }

        // --- Learned policy (heuristic scoring) ---
        if !hard_rule_hit {
            let is_question = event.content.contains('?');
            let is_message = event.event_kind == "message";
            let agent_addressed = event.content.to_lowercase().contains("agent")
                || event.content.to_lowercase().contains("assistant")
                || event.content.to_lowercase().contains("help");

            if is_message && is_question {
                action = TurnAction::Speak;
                strategy = "answer_question".into();
                confidence = 7500;
                rationale = "question detected, likely directed at agent".into();
            } else if is_message && agent_addressed {
                action = TurnAction::Speak;
                strategy = "addressed_reply".into();
                confidence = 7000;
                rationale = "agent addressed in message".into();
            } else if event.event_kind == "reaction" {
                action = TurnAction::React;
                strategy = "mirror_reaction".into();
                confidence = 6000;
                rationale = "reaction event, mirror with react".into();
            } else if is_message {
                // Non-addressed message: stay silent with moderate confidence.
                action = TurnAction::StaySilent;
                strategy = "listen".into();
                confidence = 6500;
                rationale = "message not directed at agent".into();
            } else {
                action = TurnAction::ContinuePending;
                strategy = "await_context".into();
                confidence = 5500;
                rationale = "non-message event, await more context".into();
            }
        }

        // Track consecutive turns.
        match action {
            TurnAction::Speak | TurnAction::React => {
                self.consecutive_agent_turns += 1;
                self.last_agent_reply_epoch = Some(epoch);
            }
            TurnAction::StaySilent | TurnAction::ContinuePending => {
                self.consecutive_agent_turns = 0;
            }
        }

        let decision = TurnDecision {
            epoch,
            action: action.clone(),
            strategy: strategy.clone(),
            addressee: Some(event.participant.clone()),
            confidence_basis_points: confidence,
            rationale: rationale.clone(),
        };

        // Retrieve behavioral context for the router result.
        let behavioral_context = self.retrieve_behavioral_context(&event.content)?;

        // Record the decision.
        self.record_turn_decision(&decision)?;

        // Derive signals from the event.
        self.derive_signals(event)?;

        Ok(RouterResult {
            decision,
            behavioral_context,
        })
    }

    /// Retrieve behavioral context relevant to an incoming event.
    fn retrieve_behavioral_context(&self, content: &str) -> Result<Vec<String>> {
        let mut context = Vec::new();
        // Voice card norms.
        context.extend(self.voice_card_context(content, 2)?);
        // Theory of mind for recent participants.
        context.extend(self.tom_context(content, 2)?);
        // Recent turn decisions.
        context.extend(self.turn_context(content, 2)?);
        Ok(context)
    }

    // --- Turn-taking: storage -----------------------------------------------

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
        self.recent_events.push(event.clone());
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
        // Derive signals from the new event.
        self.derive_signals(event)?;
        Ok(())
    }

    /// Retrieve recent turn context for the agent.
    pub fn turn_context(&self, query: &str, limit: u32) -> Result<Vec<String>> {
        self.search_personality(query, limit)
    }

    // --- Social signals: derivation -----------------------------------------

    /// Derive social signals from a conversation event.
    ///
    /// Computes response latency, participation balance, conversation
    /// velocity, typing-without-send, and reaction removal from the
    /// event stream.
    fn derive_signals(&mut self, event: &ConversationEvent) -> Result<()> {
        let participant = &event.participant;
        let mut signals: Vec<SocialSignal> = Vec::new();

        // Response latency: time between consecutive messages from different participants.
        if event.event_kind == "message" {
            if let Some(prev) = self.recent_events.iter().rev().nth(1) {
                if prev.participant != *participant && prev.event_kind == "message" {
                    let latency = event.epoch.saturating_sub(prev.epoch);
                    signals.push(SocialSignal {
                        signal_kind: "response_latency".into(),
                        participant: participant.clone(),
                        value: format!("{}epochs", latency),
                        epoch: event.epoch,
                    });
                }
            }
        }

        // Typing-without-send: typing event not followed by a message from same participant.
        if event.event_kind == "typing" {
            let followed_by_message = self
                .recent_events
                .iter()
                .rev()
                .take(5)
                .any(|e| e.participant == *participant && e.event_kind == "message");
            if !followed_by_message {
                signals.push(SocialSignal {
                    signal_kind: "typing_without_send".into(),
                    participant: participant.clone(),
                    value: "true".into(),
                    epoch: event.epoch,
                });
            }
        }

        // Reaction removal: deletion of a reaction.
        if event.event_kind == "reaction_removed" {
            signals.push(SocialSignal {
                signal_kind: "reaction_removal".into(),
                participant: participant.clone(),
                value: event.content.clone(),
                epoch: event.epoch,
            });
        }

        // Conversation velocity: messages per epoch window.
        let recent_msg_count = self
            .recent_events
            .iter()
            .filter(|e| e.event_kind == "message")
            .count() as f64;
        let epoch_span = event.epoch.max(1) as f64;
        let velocity = recent_msg_count / epoch_span;
        signals.push(SocialSignal {
            signal_kind: "conversation_velocity".into(),
            participant: participant.clone(),
            value: format!("{velocity:.2}"),
            epoch: event.epoch,
        });

        // Persist derived signals.
        for signal in &signals {
            self.record_signal(signal)?;
        }
        self.pending_signals
            .entry(participant.clone())
            .or_default()
            .extend(signals);

        Ok(())
    }

    /// Compute a signal summary for a participant across all recent events.
    pub fn signal_summary(&self, participant: &str) -> SignalSummary {
        let events: Vec<&ConversationEvent> = self
            .recent_events
            .iter()
            .filter(|e| e.participant == participant)
            .collect();

        let message_count = events.iter().filter(|e| e.event_kind == "message").count() as u64;
        let total_messages = self
            .recent_events
            .iter()
            .filter(|e| e.event_kind == "message")
            .count() as u64;
        let participation_share = if total_messages > 0 {
            message_count as f64 / total_messages as f64
        } else {
            0.0
        };

        // Average response latency.
        let latencies: Vec<u64> = self
            .recent_events
            .windows(2)
            .filter(|w| w[0].participant != participant && w[1].participant == participant)
            .map(|w| w[1].epoch.saturating_sub(w[0].epoch))
            .collect();
        let avg_response_latency_ms = if latencies.is_empty() {
            None
        } else {
            Some(latencies.iter().sum::<u64>() as f64 / latencies.len() as f64)
        };

        // Typing without send.
        let typing_without_send = events
            .iter()
            .filter(|e| e.event_kind == "typing")
            .filter(|te| {
                !self.recent_events.iter().any(|e| {
                    e.epoch > te.epoch
                        && e.participant == te.participant
                        && e.event_kind == "message"
                        && e.epoch <= te.epoch + 5
                })
            })
            .count() as u64;

        // Reaction count.
        let reaction_count = events.iter().filter(|e| e.event_kind == "reaction").count() as u64;

        // Conversation velocity.
        let epoch_span = self.epoch.max(1) as f64;
        let conversation_velocity = total_messages as f64 / epoch_span;

        SignalSummary {
            participant: participant.into(),
            message_count,
            avg_response_latency_ms,
            participation_share,
            conversation_velocity,
            typing_without_send,
            reaction_count,
        }
    }

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
            "Voice card v{} for \"{}\": register={}, humor={}, lexicon=[{}], banned=[{}], roles=[{}], taboos=[{}], in_jokes=[{}], norms=[{}] (confidence {}bps) evidence=[epochs {}]",
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
            card.supporting_event_epochs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        );
        let claim = ClaimInput {
            subject: format!("norms:{}", card.scope),
            predicate: "voice_card".to_string(),
            value: format!(
                "v{} register={} humor={} norms={} evidence_epochs={}",
                card.version,
                card.register,
                card.humor,
                card.group_norms.join(", "),
                card.supporting_event_epochs
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
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

    // --- Theory of mind: prediction ----------------------------------------

    /// Predict a participant's likely reception of a candidate reply and
    /// return a risk assessment.
    ///
    /// Evaluates misunderstanding, escalation, exclusion, and churn risk
    /// based on the participant's signal history and the candidate content.
    pub fn assess_risk(&self, participant: &str, candidate: &str) -> RiskAssessment {
        let summary = self.signal_summary(participant);

        // Misunderstanding risk: higher if participant has high typing-without-send
        // (suggests confusion) or low participation.
        let misunderstanding_risk = if summary.typing_without_send > 2 {
            7000
        } else if summary.participation_share < 0.2 {
            5000
        } else {
            2000
        };

        // Escalation risk: higher if candidate contains aggressive language.
        let aggressive_words = ["stupid", "wrong", "idiot", "shut up", "obviously"];
        let aggressive_count = aggressive_words
            .iter()
            .filter(|w| candidate.to_lowercase().contains(*w))
            .count();
        let escalation_risk = (aggressive_count * 3000).min(10000) as u16;

        // Exclusion risk: higher if participant has very low participation share.
        let exclusion_risk = if summary.participation_share < 0.1 {
            6000
        } else if summary.participation_share < 0.3 {
            3000
        } else {
            1000
        };

        // Churn risk: higher if participant has declining velocity or high typing-without-send.
        let churn_risk = if summary.typing_without_send > 3 {
            6500
        } else if summary.conversation_velocity < 0.1 {
            4000
        } else {
            1500
        };

        let overall = (misunderstanding_risk + escalation_risk + exclusion_risk + churn_risk) / 4;

        let recommendation = if overall > 6000 {
            RiskRecommendation::Abort
        } else if overall > 3500 {
            RiskRecommendation::Refine
        } else {
            RiskRecommendation::Proceed
        };

        RiskAssessment {
            misunderstanding_risk,
            escalation_risk,
            exclusion_risk,
            churn_risk,
            overall_risk_basis_points: overall,
            recommendation,
        }
    }

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

    /// Record a calibration result comparing a prediction to the real outcome.
    pub fn record_calibration(&mut self, record: &CalibrationRecord) -> Result<()> {
        let now = now_seconds();
        let text = format!(
            "Calibration for {} at epoch {}: predicted=\"{}\", actual=\"{}\", correct={}",
            record.participant,
            record.epoch,
            record.predicted_reaction,
            record.actual_reaction,
            record.correct,
        );
        let claim = ClaimInput {
            subject: format!("calibration:{}", record.participant),
            predicate: "calibration".to_string(),
            value: format!(
                "correct={} predicted={} actual={}",
                record.correct, record.predicted_reaction, record.actual_reaction,
            ),
            kind: ClaimKind::Fact,
            valid_from: now,
            tier: MemoryTier::LongTerm,
            processing_state: MemoryProcessingState::Processed,
        };
        self.db.remember(RememberInput {
            tenant_id: self.tenant_id.clone(),
            person_id: self.person_id.clone(),
            ingestion_key: Some(format!(
                "calibration:{}:{}",
                record.participant, record.epoch
            )),
            kind: SourceKind::Integration,
            text,
            captured_at: now,
            recorded_at: now,
            claim: Some(claim),
            feature_flag: Some(FEATURE_FLAG.into()),
        })?;
        Ok(())
    }

    // --- Personas: validation ----------------------------------------------

    /// Validate a persona blueprint for constraint satisfaction, duplicates,
    /// and empty fields.
    pub fn validate_persona(persona: &PersonaBlueprint) -> PersonaValidation {
        let mut duplicate_traits = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for trait_ in &persona.traits {
            if !seen.insert(trait_.clone()) {
                duplicate_traits.push(trait_.clone());
            }
        }

        let mut empty_fields = Vec::new();
        if persona.name.trim().is_empty() {
            empty_fields.push("name".into());
        }
        if persona.system_prompt.trim().is_empty() {
            empty_fields.push("system_prompt".into());
        }
        if persona.traits.is_empty() {
            empty_fields.push("traits".into());
        }

        // Check constraint satisfaction: no constraint should contradict a trait.
        let constraints_satisfied = persona.constraints.iter().all(|c| {
            // A constraint like "never X" should not have a trait that says "always X".
            !persona.traits.iter().any(|t| {
                let t_lower = t.to_lowercase();
                let c_lower = c.to_lowercase();
                (c_lower.contains("never")
                    && t_lower.contains(c_lower.replace("never ", "").as_str()))
                    || (c_lower.contains("always") && t_lower.contains("never"))
            })
        });

        let valid = empty_fields.is_empty() && duplicate_traits.is_empty() && constraints_satisfied;

        PersonaValidation {
            name: persona.name.clone(),
            constraints_satisfied,
            trait_count: persona.traits.len(),
            duplicate_traits,
            empty_fields,
            valid,
        }
    }

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

    // --- Social observability: analysis ------------------------------------

    /// Analyze a conversation window and produce a health summary with
    /// findings.
    ///
    /// This runs over the recent event buffer, computing participation
    /// balance, error rate, and generating evidence-cited findings.
    pub fn analyze_conversation(&mut self, scope: &str) -> Result<ConversationHealth> {
        let events = &self.recent_events;
        let total_turns = events.len() as u64;
        let agent_turns = events.iter().filter(|e| e.participant == "agent").count() as u64;
        let user_turns = events.iter().filter(|e| e.participant != "agent").count() as u64;

        let participation_balance = if total_turns > 0 {
            let agent_share = agent_turns as f64 / total_turns as f64;
            1.0 - (agent_share - 0.5).abs() * 2.0
        } else {
            0.0
        };

        let error_rate = events.iter().filter(|e| e.event_kind == "error").count() as f64
            / total_turns.max(1) as f64;

        let mut findings = Vec::new();

        // Finding: participation imbalance.
        if participation_balance < 0.3 {
            findings.push(ObservationFinding {
                scope: scope.into(),
                finding: "Participation is heavily imbalanced — one party dominates".into(),
                evidence: vec![format!(
                    "agent={}/{} user={}/{} balance={:.2}",
                    agent_turns, total_turns, user_turns, total_turns, participation_balance
                )],
                recommendation: Some("adjust turn-taking thresholds".into()),
                severity: ObservationSeverity::Warning,
            });
        }

        // Finding: high error rate.
        if error_rate > 0.2 {
            findings.push(ObservationFinding {
                scope: scope.into(),
                finding: format!("High error rate: {:.0}%", error_rate * 100.0),
                evidence: vec![format!(
                    "{} errors out of {} events",
                    events.iter().filter(|e| e.event_kind == "error").count(),
                    total_turns
                )],
                recommendation: Some("review tool configuration and retry strategy".into()),
                severity: if error_rate > 0.5 {
                    ObservationSeverity::Critical
                } else {
                    ObservationSeverity::Warning
                },
            });
        }

        // Finding: conversation velocity.
        let epoch_span = self.epoch.max(1) as f64;
        let velocity = total_turns as f64 / epoch_span;
        if velocity < 0.1 && total_turns > 5 {
            findings.push(ObservationFinding {
                scope: scope.into(),
                finding: "Conversation velocity is low — long gaps between events".into(),
                evidence: vec![format!(
                    "{total_turns} events over {epoch_span} epochs (velocity {velocity:.2})"
                )],
                recommendation: Some("consider proactive engagement".into()),
                severity: ObservationSeverity::Info,
            });
        }

        // Finding: typing without send.
        let typing_without_send_count = events.iter().filter(|e| e.event_kind == "typing").count();
        if typing_without_send_count > 3 {
            findings.push(ObservationFinding {
                scope: scope.into(),
                finding: "Multiple typing-without-send events detected — possible confusion".into(),
                evidence: vec![format!(
                    "{typing_without_send_count} typing events without follow-up messages"
                )],
                recommendation: Some("simplify responses and check for misunderstanding".into()),
                severity: ObservationSeverity::Warning,
            });
        }

        // Persist findings.
        for finding in &findings {
            self.record_finding(finding)?;
        }

        Ok(ConversationHealth {
            scope: scope.into(),
            total_turns,
            agent_turns,
            user_turns,
            participation_balance,
            error_rate,
            findings,
        })
    }

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
    ///
    /// This is called automatically by the rotary agent loop before each
    /// turn to proactively inject behavioral context.
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

    // --- Turn router tests -------------------------------------------------

    #[test]
    fn router_replies_to_direct_mention() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        let result = personality
            .route_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "hey @agent can you help?".into(),
            })
            .unwrap();

        assert_eq!(result.decision.action, TurnAction::Speak);
        assert_eq!(result.decision.strategy, "direct_reply");
        assert!(result.decision.confidence_basis_points >= 9000);
    }

    #[test]
    fn router_replies_to_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        let result = personality
            .route_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "/help".into(),
            })
            .unwrap();

        assert_eq!(result.decision.action, TurnAction::Speak);
        assert_eq!(result.decision.strategy, "command_response");
    }

    #[test]
    fn router_stays_silent_for_unaddressed_messages() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        let result = personality
            .route_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "just chatting with friends".into(),
            })
            .unwrap();

        assert_eq!(result.decision.action, TurnAction::StaySilent);
        assert_eq!(result.decision.strategy, "listen");
    }

    #[test]
    fn router_reacts_to_reaction_events() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        let result = personality
            .route_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "reaction".into(),
                content: "thumbs_up".into(),
            })
            .unwrap();

        assert_eq!(result.decision.action, TurnAction::React);
        assert_eq!(result.decision.strategy, "mirror_reaction");
    }

    #[test]
    fn router_answers_questions() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        let result = personality
            .route_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "what is the weather?".into(),
            })
            .unwrap();

        assert_eq!(result.decision.action, TurnAction::Speak);
        assert_eq!(result.decision.strategy, "answer_question");
    }

    #[test]
    fn router_enforces_consecutive_turn_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id).with_rules(RouterRules {
            max_consecutive_turns: 2,
            ..Default::default()
        });

        // Force 3 consecutive speaks.
        for i in 1..=3 {
            let result = personality
                .route_event(&ConversationEvent {
                    epoch: i,
                    participant: "user".into(),
                    event_kind: "message".into(),
                    content: "hey @agent".into(),
                })
                .unwrap();
            if i <= 2 {
                assert_eq!(result.decision.action, TurnAction::Speak);
            } else {
                assert_eq!(result.decision.action, TurnAction::StaySilent);
                assert_eq!(result.decision.strategy, "consecutive_limit");
            }
        }
    }

    #[test]
    fn router_enforces_rate_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id).with_rules(RouterRules {
            min_reply_interval: 5,
            ..Default::default()
        });

        // First reply at epoch 1.
        let r1 = personality
            .route_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "hey @agent".into(),
            })
            .unwrap();
        assert_eq!(r1.decision.action, TurnAction::Speak);

        // Second at epoch 2 — should be rate limited.
        let r2 = personality
            .route_event(&ConversationEvent {
                epoch: 2,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "hey @agent again".into(),
            })
            .unwrap();
        assert_eq!(r2.decision.action, TurnAction::StaySilent);
        assert_eq!(r2.decision.strategy, "rate_limited");
    }

    #[test]
    fn router_returns_behavioral_context() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        // Store a voice card first.
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
                supporting_event_epochs: vec![1],
                valid_from: 0,
                valid_until: None,
            })
            .unwrap();

        let result = personality
            .route_event(&ConversationEvent {
                epoch: 2,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "voice card norms general".into(),
            })
            .unwrap();

        // Behavioral context should include the voice card.
        assert!(!result.behavioral_context.is_empty());
    }

    // --- Signal derivation tests -------------------------------------------

    #[test]
    fn derives_response_latency_signal() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        // User sends message at epoch 1.
        personality
            .record_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "hello".into(),
            })
            .unwrap();

        // Agent replies at epoch 3.
        personality
            .record_event(&ConversationEvent {
                epoch: 3,
                participant: "agent".into(),
                event_kind: "message".into(),
                content: "hi there".into(),
            })
            .unwrap();

        let summary = personality.signal_summary("agent");
        assert!(summary.avg_response_latency_ms.is_some());
    }

    #[test]
    fn derives_typing_without_send_signal() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        // User types but never sends.
        personality
            .record_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "typing".into(),
                content: "".into(),
            })
            .unwrap();

        let summary = personality.signal_summary("user");
        assert!(summary.typing_without_send >= 1);
    }

    #[test]
    fn derives_conversation_velocity() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        for i in 1..=5 {
            personality
                .record_event(&ConversationEvent {
                    epoch: i,
                    participant: "user".into(),
                    event_kind: "message".into(),
                    content: format!("msg {i}"),
                })
                .unwrap();
        }

        let summary = personality.signal_summary("user");
        assert!(summary.conversation_velocity > 0.0);
        assert_eq!(summary.message_count, 5);
    }

    #[test]
    fn signal_summary_computes_participation_share() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        // 3 user messages, 1 agent message.
        personality
            .record_event(&ConversationEvent {
                epoch: 1,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "a".into(),
            })
            .unwrap();
        personality
            .record_event(&ConversationEvent {
                epoch: 2,
                participant: "agent".into(),
                event_kind: "message".into(),
                content: "b".into(),
            })
            .unwrap();
        personality
            .record_event(&ConversationEvent {
                epoch: 3,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "c".into(),
            })
            .unwrap();
        personality
            .record_event(&ConversationEvent {
                epoch: 4,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "d".into(),
            })
            .unwrap();

        let summary = personality.signal_summary("user");
        assert_eq!(summary.message_count, 3);
        assert!((summary.participation_share - 0.75).abs() < 0.01);
    }

    // --- Theory of mind: risk assessment tests -----------------------------

    #[test]
    fn risk_assessment_proceeds_for_safe_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        // Give the user some normal participation.
        for i in 1..=5 {
            personality
                .record_event(&ConversationEvent {
                    epoch: i,
                    participant: "user".into(),
                    event_kind: "message".into(),
                    content: format!("msg {i}"),
                })
                .unwrap();
        }

        let risk = personality.assess_risk("user", "Here is a helpful answer.");
        assert_eq!(risk.recommendation, RiskRecommendation::Proceed);
        assert!(risk.overall_risk_basis_points < 3500);
    }

    #[test]
    fn risk_assessment_refines_for_aggressive_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        for i in 1..=5 {
            personality
                .record_event(&ConversationEvent {
                    epoch: i,
                    participant: "user".into(),
                    event_kind: "message".into(),
                    content: format!("msg {i}"),
                })
                .unwrap();
        }

        let risk = personality.assess_risk("user", "That is stupid and wrong.");
        assert!(risk.escalation_risk >= 3000);
    }

    #[test]
    fn risk_assessment_flags_excluded_participant() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        // Only agent messages — user has 0 participation.
        for i in 1..=5 {
            personality
                .record_event(&ConversationEvent {
                    epoch: i,
                    participant: "agent".into(),
                    event_kind: "message".into(),
                    content: format!("agent msg {i}"),
                })
                .unwrap();
        }

        let risk = personality.assess_risk("quiet_user", "Hello there.");
        assert!(risk.exclusion_risk >= 6000);
    }

    #[test]
    fn calibration_records_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        personality
            .record_calibration(&CalibrationRecord {
                participant: "alice".into(),
                predicted_reaction: "satisfied".into(),
                actual_reaction: "frustrated".into(),
                correct: false,
                epoch: 5,
            })
            .unwrap();

        let context = personality
            .search_personality("calibration alice", 5)
            .unwrap();
        assert_eq!(context.len(), 1);
        assert!(context[0].contains("frustrated"));
    }

    // --- Persona validation tests ------------------------------------------

    #[test]
    fn validates_good_persona() {
        let persona = PersonaBlueprint {
            name: "helpful".into(),
            traits: vec!["patient".into(), "thorough".into()],
            system_prompt: "You are helpful.".into(),
            constraints: vec!["never rush".into()],
            citations: vec![],
        };
        let validation = Personality::validate_persona(&persona);
        assert!(validation.valid);
        assert!(validation.duplicate_traits.is_empty());
        assert!(validation.empty_fields.is_empty());
    }

    #[test]
    fn rejects_duplicate_traits() {
        let persona = PersonaBlueprint {
            name: "dup".into(),
            traits: vec!["patient".into(), "patient".into()],
            system_prompt: "You are patient.".into(),
            constraints: vec![],
            citations: vec![],
        };
        let validation = Personality::validate_persona(&persona);
        assert!(!validation.valid);
        assert_eq!(validation.duplicate_traits, vec!["patient"]);
    }

    #[test]
    fn rejects_empty_fields() {
        let persona = PersonaBlueprint {
            name: "".into(),
            traits: vec![],
            system_prompt: "".into(),
            constraints: vec![],
            citations: vec![],
        };
        let validation = Personality::validate_persona(&persona);
        assert!(!validation.valid);
        assert!(validation.empty_fields.contains(&"name".to_string()));
        assert!(
            validation
                .empty_fields
                .contains(&"system_prompt".to_string())
        );
        assert!(validation.empty_fields.contains(&"traits".to_string()));
    }

    // --- Observability analysis tests --------------------------------------

    #[test]
    fn analyze_conversation_detects_imbalance() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        // 9 agent messages, 1 user message.
        for i in 1..=9 {
            personality
                .record_event(&ConversationEvent {
                    epoch: i,
                    participant: "agent".into(),
                    event_kind: "message".into(),
                    content: format!("agent {i}"),
                })
                .unwrap();
        }
        personality
            .record_event(&ConversationEvent {
                epoch: 10,
                participant: "user".into(),
                event_kind: "message".into(),
                content: "ok".into(),
            })
            .unwrap();

        let health = personality.analyze_conversation("thread-1").unwrap();
        assert!(health.participation_balance < 0.3);
        assert!(
            health
                .findings
                .iter()
                .any(|f| f.finding.contains("imbalanced"))
        );
    }

    #[test]
    fn analyze_conversation_detects_high_error_rate() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        for i in 1..=10 {
            personality
                .record_event(&ConversationEvent {
                    epoch: i,
                    participant: "agent".into(),
                    event_kind: if i % 3 == 0 { "error" } else { "message" }.to_string(),
                    content: format!("event {i}"),
                })
                .unwrap();
        }

        let health = personality.analyze_conversation("thread-2").unwrap();
        assert!(health.error_rate > 0.2);
        assert!(
            health
                .findings
                .iter()
                .any(|f| f.finding.contains("error rate"))
        );
    }

    #[test]
    fn analyze_conversation_detects_typing_without_send() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        for i in 1..=5 {
            personality
                .record_event(&ConversationEvent {
                    epoch: i,
                    participant: "user".into(),
                    event_kind: "typing".into(),
                    content: "".into(),
                })
                .unwrap();
        }

        let health = personality.analyze_conversation("thread-3").unwrap();
        assert!(
            health
                .findings
                .iter()
                .any(|f| f.finding.contains("typing-without-send"))
        );
    }

    #[test]
    fn analyze_conversation_produces_clean_health_for_balanced_convo() {
        let tmp = tempfile::tempdir().unwrap();
        let db = MemoryDb::open(tmp.path().join("personality.db")).unwrap();
        let (tenant_id, person_id) = test_ids();
        let mut personality = Personality::new(db, tenant_id, person_id);

        // Balanced conversation.
        for i in 1..=10 {
            personality
                .record_event(&ConversationEvent {
                    epoch: i,
                    participant: if i % 2 == 0 { "user" } else { "agent" }.to_string(),
                    event_kind: "message".into(),
                    content: format!("msg {i}"),
                })
                .unwrap();
        }

        let health = personality.analyze_conversation("thread-4").unwrap();
        assert!(health.participation_balance > 0.8);
        assert!(health.findings.is_empty());
    }

    // --- Existing storage/retrieval tests ----------------------------------

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
                supporting_event_epochs: vec![1, 2],
                valid_from: 0,
                valid_until: None,
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
                supporting_event_epochs: vec![],
                valid_from: 0,
                valid_until: None,
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
