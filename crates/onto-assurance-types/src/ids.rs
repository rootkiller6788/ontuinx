//! Typed identifiers — newtype wrappers over UUIDv7.
//!
//! Every domain object gets its own ID type.  No raw strings or bare UUIDs
//! cross module boundaries.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! typed_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a new unique ID.  Only lifecycle-owning modules
            /// (run creation, attempt creation) should call this.  Bridge
            /// and adapter code must use [`parse`](Self::parse) or
            /// [`from_uuid`](Self::from_uuid) to recover an existing
            /// identity — never manufacture one.
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            /// Parse from a string representation.  Use at protocol
            /// boundaries (Bridge, adapter, store) to recover a
            /// previously-created identity.  Fails on malformed input
            /// rather than silently fabricating a new ID.
            pub fn parse(value: &str) -> Result<Self, uuid::Error> {
                uuid::Uuid::parse_str(value).map(Self)
            }

            /// Deprecated alias for [`generate`](Self::generate).
            /// Bridge/adapter code must use [`parse`](Self::parse) instead.
            #[deprecated(note = "use generate() for new entities, parse() at protocol boundaries")]
            pub fn new() -> Self {
                Self::generate()
            }

            /// Create from an already-parsed UUID.
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Borrow the inner UUID.
            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Consume and return the inner UUID.
            pub fn into_uuid(self) -> Uuid {
                self.0
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = uuid::Error;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = uuid::Error;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::generate()
            }
        }
    };
}

typed_id!(RunId, "Unique identifier for a single agent run / task execution.");
typed_id!(AttemptId, "Identifier for one attempt within a run.");
typed_id!(TransactionId, "Identifier for one side-effect-bearing transaction.");
typed_id!(ContractId, "Identifier for an ExecutionContract instance.");
typed_id!(IntentId, "Identifier for an ExecutionIntent.");
typed_id!(CriterionId, "Identifier for a single AcceptanceCriterion.");
typed_id!(EvidenceId, "Identifier for one EvidenceRecord.");
typed_id!(BundleId, "Identifier for an EvidenceBundle.");
typed_id!(CheckpointId, "Identifier for a CheckpointBinding.");
typed_id!(DecisionId, "Identifier for a session or attempt decision.");
typed_id!(ApprovalId, "Identifier for an approval request.");
typed_id!(VerifierId, "Identifier for a VerifierBinding.");
typed_id!(CorrelationId, "Groups related transactions across attempts.");
typed_id!(CausationId, "Points to the event that caused this transaction.");
typed_id!(PublishReceiptId, "Identifier for a publish receipt.");
typed_id!(InvocationId, "Identifier for a single capability invocation.");
typed_id!(CapabilityId, "Identifier for a registered capability.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let a = RunId::new();
        let b = RunId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn different_types_are_incompatible() {
        let rid = RunId::new();
        let tid = TransactionId::new();
        // Would not compile: assert_ne!(rid, tid);
        let _ = (rid, tid);
    }

    #[test]
    fn serde_roundtrip() {
        let id = RunId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: RunId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
