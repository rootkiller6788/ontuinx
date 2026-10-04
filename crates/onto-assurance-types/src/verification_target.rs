//! VerificationTarget — 通用验证目标。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VerificationTarget {
    pub target_id: String,
    pub target_kind: TargetKind,
    pub target_ref: String,
    pub content_hash: String,
    pub language: Option<String>,
    pub size_bytes: Option<u64>,
    pub line_count: Option<u32>,
    #[serde(default)]
    pub risk_level: TargetRiskLevel,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}
impl VerificationTarget {
    pub fn new(id: &str, kind: TargetKind, ref_path: &str, hash: &str) -> Self {
        Self { target_id: id.into(), target_kind: kind, target_ref: ref_path.into(), content_hash: hash.into(), ..Default::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TargetRiskLevel { #[default] Standard, High, Critical }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind { #[default] SourceFile, Document, Dataset, Workflow, Resource, Custom(String) }
