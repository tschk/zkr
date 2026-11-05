use crate::{MemoryTier, SourceKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMode {
    Coding,
    #[default]
    Personal,
    Research,
    Agent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeBoundary {
    Workspace,
    Person,
    Project,
    Agent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryPreset {
    pub mode: MemoryMode,
    pub scope: ScopeBoundary,
    pub default_tier: MemoryTier,
    pub retrieval_limit: u32,
    pub max_retrieval_limit: u32,
    pub citations_required: bool,
    pub scope_isolated: bool,
    pub profile_memory: bool,
    pub allowed_source_kinds: Vec<SourceKind>,
}

impl MemoryMode {
    pub fn preset(self) -> MemoryPreset {
        let (
            scope,
            default_tier,
            retrieval_limit,
            max_retrieval_limit,
            profile_memory,
            allowed_source_kinds,
        ) = match self {
            Self::Coding => (
                ScopeBoundary::Workspace,
                MemoryTier::LongTerm,
                10,
                20,
                false,
                vec![
                    SourceKind::Conversation,
                    SourceKind::Document,
                    SourceKind::Integration,
                    SourceKind::UserCorrection,
                ],
            ),
            Self::Personal => (
                ScopeBoundary::Person,
                MemoryTier::LongTerm,
                10,
                20,
                true,
                vec![
                    SourceKind::Conversation,
                    SourceKind::Screen,
                    SourceKind::Audio,
                    SourceKind::Document,
                    SourceKind::Integration,
                    SourceKind::UserCorrection,
                ],
            ),
            Self::Research => (
                ScopeBoundary::Project,
                MemoryTier::LongTerm,
                20,
                50,
                false,
                vec![
                    SourceKind::Conversation,
                    SourceKind::Document,
                    SourceKind::Integration,
                    SourceKind::UserCorrection,
                ],
            ),
            Self::Agent => (
                ScopeBoundary::Agent,
                MemoryTier::ShortTerm,
                8,
                20,
                false,
                vec![
                    SourceKind::Conversation,
                    SourceKind::Integration,
                    SourceKind::UserCorrection,
                ],
            ),
        };
        MemoryPreset {
            mode: self,
            scope,
            default_tier,
            retrieval_limit,
            max_retrieval_limit,
            citations_required: true,
            scope_isolated: true,
            profile_memory,
            allowed_source_kinds,
        }
    }
}

impl MemoryPreset {
    pub fn allows_source(&self, kind: &SourceKind) -> bool {
        self.allowed_source_kinds.contains(kind)
    }

    pub fn retrieval_limit(&self, requested: Option<u32>) -> u32 {
        requested
            .unwrap_or(self.retrieval_limit)
            .clamp(1, self.max_retrieval_limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_apply_scoped_defaults_and_bounds() {
        let coding = MemoryMode::Coding.preset();
        assert_eq!(coding.scope, ScopeBoundary::Workspace);
        assert_eq!(coding.default_tier, MemoryTier::LongTerm);
        assert!(coding.citations_required);
        assert!(coding.scope_isolated);
        assert!(!coding.profile_memory);
        assert!(coding.allows_source(&SourceKind::Document));
        assert!(!coding.allows_source(&SourceKind::Screen));
        assert_eq!(coding.retrieval_limit(None), 10);
        assert_eq!(coding.retrieval_limit(Some(0)), 1);
        assert_eq!(coding.retrieval_limit(Some(100)), 20);

        let personal = MemoryMode::Personal.preset();
        assert_eq!(personal.scope, ScopeBoundary::Person);
        assert!(personal.profile_memory);
        assert!(personal.allows_source(&SourceKind::Audio));

        let research = MemoryMode::Research.preset();
        assert_eq!(research.scope, ScopeBoundary::Project);
        assert_eq!(research.retrieval_limit(None), 20);
        assert_eq!(research.retrieval_limit(Some(100)), 50);

        let agent = MemoryMode::Agent.preset();
        assert_eq!(agent.scope, ScopeBoundary::Agent);
        assert_eq!(agent.default_tier, MemoryTier::ShortTerm);
        assert!(!agent.allows_source(&SourceKind::Document));
    }

    #[test]
    fn mode_names_are_stable_configuration_values() {
        assert_eq!(
            serde_json::to_string(&MemoryMode::Coding).unwrap(),
            "\"coding\""
        );
        assert_eq!(
            serde_json::from_str::<MemoryMode>("\"agent\"").unwrap(),
            MemoryMode::Agent
        );
        assert_eq!(MemoryMode::default(), MemoryMode::Personal);
    }
}
