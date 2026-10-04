//! Rules — OCR 移植的 26 种文件类型路由 + 25 个语言规则文件 + Go 新增。
//!
//! 三层简单结构：
//! ① system_rules.json  — 路由表 (glob → rule_name)
//! ② rule_docs/*.md     — 规则内容 (LLM 直接读的自然语言)
//! ③ router.rs          — 确定性路径→规则解析

pub mod router;
