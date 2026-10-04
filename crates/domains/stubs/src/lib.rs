//! Future domain stubs — prove the DomainPack trait is extensible.
//!
//! Each module is a 15-line DomainPack implementation.
//! Production domains get their own crate (see onto-assurance-domain-code).
//! These stubs exist only to validate the SPI contract.
//!
//! To create a real domain plugin:
//!   1. Copy a stub module
//!   2. Implement actual verifiers
//!   3. Move to its own crate (onto-assurance-domain-{name})

use onto_assurance_types::finding::FindingCandidate;
use onto_assurance_types::verification_plan::VerificationUnit;
use onto_pack::DomainPack;

macro_rules! domain_stub {
    ($name:ident, $id:literal, $label:literal, $kinds:expr) => {
        pub mod $name {
            use super::*;
            pub struct Domain;
            impl DomainPack for Domain {
                fn domain_id(&self) -> &str { $id }
                fn domain_name(&self) -> &str { $label }
                fn supported_kinds(&self) -> Vec<String> { $kinds }
                fn verify(&self, _: &VerificationUnit) -> Result<Vec<FindingCandidate>, String> { Ok(vec![]) }
            }
            #[cfg(test)]
            mod tests {
                use super::*;
                use onto_pack::DomainPack as _;
                #[test]
                fn stub_registers() { assert_eq!(Domain.domain_id(), $id); }
            }
        }
    };
}

domain_stub!(robotics,      "robotics",      "Robotics Assurance Pack",      vec![]);
domain_stub!(chip,          "chip",          "Chip Assurance Pack",          vec![]);
domain_stub!(automation,    "automation",    "Automation Assurance Pack",    vec![]);
domain_stub!(automotive,    "automotive",    "Automotive Assurance Pack",    vec![]);
domain_stub!(aerospace,     "aerospace",     "Aerospace Assurance Pack",     vec![]);
domain_stub!(medical,       "medical",       "Medical Assurance Pack",       vec![]);
domain_stub!(energy,        "energy",        "Energy Assurance Pack",        vec![]);
domain_stub!(chemical,      "chemical",      "Chemical Assurance Pack",      vec![]);
domain_stub!(civil,         "civil",         "Civil Assurance Pack",         vec![]);
domain_stub!(network,       "network",       "Network Assurance Pack",       vec![]);
domain_stub!(crypto,        "crypto",        "Crypto Assurance Pack",        vec![]);
domain_stub!(ml,            "ml",            "ML Assurance Pack",            vec![]);
domain_stub!(iot,           "iot",           "IoT Assurance Pack",           vec![]);
domain_stub!(simulation,    "simulation",    "Simulation Assurance Pack",    vec![]);
domain_stub!(supply_chain,  "supply_chain",  "Supply Chain Assurance Pack",  vec![]);
domain_stub!(quantum,       "quantum",       "Quantum Assurance Pack",       vec![]);
domain_stub!(bio,           "bio",           "Bio Assurance Pack",           vec![]);
domain_stub!(document,      "document",      "Document Assurance Pack",      vec!["document".into()]);
domain_stub!(workflow,      "workflow",      "Workflow Assurance Pack",      vec!["workflow".into()]);
domain_stub!(data,          "data",          "Data Assurance Pack",          vec!["dataset".into(), "schema".into()]);
