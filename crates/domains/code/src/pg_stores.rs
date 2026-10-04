//! PG-backed Verification Stores (S1)。
//!
//! 替换内存 Mock，使用真实 PostgreSQL 持久化。

use onto_assurance_types::verification_ledger::{
    ScopeLedger, RuleCoverageLedger, VerifierExecutionLedger,
};
use onto_assurance_types::verification_session::VerificationSession;
use onto_assurance_types::verifier_run_result::VerifierRunResult;

/// PostgreSQL 连接配置。
#[derive(Debug, Clone)]
pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub dbname: String,
}

impl Default for PgConfig {
    fn default() -> Self {
        Self {
            host: "localhost".into(), port: 5432, user: "postgres".into(),
            password: "postgres".into(), dbname: "postgres".into(),
        }
    }
}

/// PG 持久化 Store 集合。
pub struct PgVerificationStores {
    config: PgConfig,
}

impl PgVerificationStores {
    pub fn new(config: PgConfig) -> Self { Self { config } }

    fn conn_string(&self) -> String {
        format!("host={} port={} user={} password={} dbname={} sslmode=disable",
            self.config.host, self.config.port, self.config.user, self.config.password, self.config.dbname)
    }

    /// 确保表存在。
    pub fn ensure_tables(&self) -> Result<(), String> {
        let output = std::process::Command::new("psql")
            .args(["-c", "CREATE TABLE IF NOT EXISTS vf_sessions (
                session_id TEXT PRIMARY KEY,
                plan_json JSONB NOT NULL,
                state TEXT NOT NULL,
                completed_units JSONB DEFAULT '[]',
                failed_units JSONB DEFAULT '[]',
                findings JSONB DEFAULT '[]',
                tokens_used BIGINT DEFAULT 0,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                updated_at TIMESTAMPTZ DEFAULT NOW()
            )"])
            .env("PGPASSWORD", &self.config.password)
            .env("PGUSER", &self.config.user)
            .output().map_err(|e| e.to_string())?;
        if !output.status.success() { return Err(String::from_utf8_lossy(&output.stderr).into()); }

        let output = std::process::Command::new("psql")
            .args(["-c", "CREATE TABLE IF NOT EXISTS vf_verifier_results (
                run_id TEXT PRIMARY KEY,
                unit_id TEXT NOT NULL,
                verifier_id TEXT NOT NULL,
                execution_status TEXT NOT NULL,
                verifier_verdict TEXT,
                findings_json JSONB DEFAULT '[]',
                checkpoint_hash TEXT,
                exit_code INT,
                started_at TEXT,
                finished_at TEXT,
                created_at TIMESTAMPTZ DEFAULT NOW()
            )"])
            .env("PGPASSWORD", &self.config.password).env("PGUSER", &self.config.user)
            .output().map_err(|e| e.to_string())?;
        if !output.status.success() { return Err(String::from_utf8_lossy(&output.stderr).into()); }

        let output = std::process::Command::new("psql")
            .args(["-c", "CREATE TABLE IF NOT EXISTS vf_scope_ledger (
                target_id TEXT PRIMARY KEY,
                disposition TEXT NOT NULL,
                recorded_at TIMESTAMPTZ DEFAULT NOW()
            )"])
            .env("PGPASSWORD", &self.config.password).env("PGUSER", &self.config.user)
            .output().map_err(|e| e.to_string())?;
        if !output.status.success() { return Err(String::from_utf8_lossy(&output.stderr).into()); }

        Ok(())
    }

    /// 保存 VerificationSession。
    pub fn save_session(&self, session: &VerificationSession) -> Result<(), String> {
        let json = serde_json::to_string(&session.plan).map_err(|e| e.to_string())?;
        let completed = serde_json::to_string(&session.completed_units).unwrap_or_default();
        let failed = serde_json::to_string(&session.failed_units).unwrap_or_default();
        let findings = serde_json::to_string(&session.findings).unwrap_or_default();
        let sql = format!(
            "INSERT INTO vf_sessions (session_id, plan_json, state, completed_units, failed_units, findings, tokens_used, updated_at)
             VALUES ('{}', '{}', '{:?}', '{}', '{}', '{}', {}, NOW())
             ON CONFLICT (session_id) DO UPDATE SET state='{:?}', completed_units='{}', updated_at=NOW()",
            session.session_id, json, session.state, completed, failed, findings, session.tokens_used, session.state, completed
        );
        let output = std::process::Command::new("psql")
            .args(["-c", &sql])
            .env("PGPASSWORD", &self.config.password).env("PGUSER", &self.config.user)
            .output().map_err(|e| e.to_string())?;
        if !output.status.success() { return Err(String::from_utf8_lossy(&output.stderr).into()); }
        Ok(())
    }

    /// 保存 VerifierRunResult。
    pub fn save_result(&self, result: &VerifierRunResult) -> Result<(), String> {
        let findings = serde_json::to_string(&result.findings).unwrap_or_default();
        let sql = format!(
            "INSERT INTO vf_verifier_results (run_id, unit_id, verifier_id, execution_status, verifier_verdict, findings_json, checkpoint_hash, exit_code, started_at, finished_at)
             VALUES ('{}', '{}', '{}', '{:?}', '{}', '{}', '{}', {}, '{}', '{}')
             ON CONFLICT (run_id) DO NOTHING",
            result.run_id, result.unit_id, result.verifier_id,
            result.execution_status,
            result.verifier_verdict.as_ref().map(|v| format!("{:?}", v)).unwrap_or_default(),
            findings, result.checkpoint_hash,
            result.exit_code.unwrap_or(-1),
            result.started_at, result.finished_at
        );
        let output = std::process::Command::new("psql")
            .args(["-c", &sql])
            .env("PGPASSWORD", &self.config.password).env("PGUSER", &self.config.user)
            .output().map_err(|e| e.to_string())?;
        if !output.status.success() { return Err(String::from_utf8_lossy(&output.stderr).into()); }
        Ok(())
    }

    /// 记录 Scope 处置。
    pub fn record_scope(&self, target_id: &str, disposition: &str) -> Result<(), String> {
        let sql = format!(
            "INSERT INTO vf_scope_ledger (target_id, disposition) VALUES ('{}', '{}') ON CONFLICT (target_id) DO UPDATE SET disposition='{}'",
            target_id, disposition, disposition
        );
        let output = std::process::Command::new("psql")
            .args(["-c", &sql])
            .env("PGPASSWORD", &self.config.password).env("PGUSER", &self.config.user)
            .output().map_err(|e| e.to_string())?;
        if !output.status.success() { return Err(String::from_utf8_lossy(&output.stderr).into()); }
        Ok(())
    }

    /// 验证 PG 连接可用。
    pub fn health_check(&self) -> Result<bool, String> {
        let output = std::process::Command::new("psql")
            .args(["-c", "SELECT 1"])
            .env("PGPASSWORD", &self.config.password).env("PGUSER", &self.config.user)
            .output().map_err(|e| e.to_string())?;
        Ok(output.status.success())
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let cfg = PgConfig::default();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 5432);
    }

    #[test]
    fn pg_health_check() {
        let stores = PgVerificationStores::new(PgConfig::default());
        match stores.health_check() {
            Ok(true) => { /* PG available */ }
            _ => { /* PG not available in this environment */ }
        }
    }

    #[test]
    fn ensure_tables_creates() {
        let stores = PgVerificationStores::new(PgConfig::default());
        if stores.health_check().unwrap_or(false) {
            assert!(stores.ensure_tables().is_ok());
            // Cleanup
            let _ = std::process::Command::new("psql")
                .args(["-c", "DROP TABLE IF EXISTS vf_sessions, vf_verifier_results, vf_scope_ledger"])
                .env("PGPASSWORD", "postgres").env("PGUSER", "postgres")
                .output();
        }
    }
}
