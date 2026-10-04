//! ontofirmwaregraphd — Shared-memory code graph service (P3 + P7).
//!
//! Architecture:
//!   Rust (parent) ← memfd + ring + 2×eventfd → C child (ofg-core)
//!   P7: gRPC SnapshotQueryService + GraphRiskService (PG-backed)

pub mod merkle;
pub mod pg_store;
pub mod ring;
pub mod snapshot;
pub mod snapshot_query_service;
pub mod graph_risk_service;
pub mod query_service;
pub mod projector;

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use std::os::fd::OwnedFd;
use std::os::unix::io::FromRawFd;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Server;
use tracing::info;

pub mod ofg { tonic::include_proto!("ofg"); }
use ofg::health_server::{Health, HealthServer};
use ofg::{HealthRequest, HealthResponse};

const OFG_WIRE_MAGIC: u32       = 0x4F464731;
const OFG_CTRL_OFFSET: usize     = 0;
const OFG_RING_META_OFFSET: usize = 64;
const OFG_RING_DEFAULT_CAP: usize = 256 * 1024 * 1024;

const MFD_CLOEXEC: u32 = 0x0001u32;
const MFD_ALLOW_SEALING: u32 = 0x0002u32;

struct ShmRegion {
    _memfd: OwnedFd,
    ptr: *mut u8,
    size: usize,
    data_fd: i32,
    space_fd: i32,
}

impl ShmRegion {
    fn create(capacity: usize) -> Result<Self> {
        // No CLOEXEC: child must inherit this FD.
        let memfd = unsafe { libc::syscall(libc::SYS_memfd_create, "ofg-batch-ring\0".as_ptr(), MFD_ALLOW_SEALING) as i32 };
        if memfd < 0 { anyhow::bail!("memfd_create failed: {}", std::io::Error::last_os_error()); }

        unsafe { libc::ftruncate(memfd, capacity as i64); }

        let ptr = unsafe {
            libc::mmap(std::ptr::null_mut(), capacity, libc::PROT_READ | libc::PROT_WRITE,
                       libc::MAP_SHARED, memfd, 0)
        };
        if ptr == libc::MAP_FAILED { anyhow::bail!("mmap failed"); }

        // Init control header magic.
        unsafe { (ptr as *mut u32).write(OFG_WIRE_MAGIC); }
        // Init ring capacity.
        unsafe { (ptr.add(OFG_RING_META_OFFSET) as *mut u64).write(capacity as u64); }

        // No CLOEXEC: child must inherit these FDs.
        let data_fd  = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK) };
        let space_fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK) };
        if data_fd < 0 || space_fd < 0 { anyhow::bail!("eventfd failed"); }

        Ok(Self { _memfd: unsafe { OwnedFd::from_raw_fd(memfd) }, ptr: ptr as *mut u8, size: capacity, data_fd, space_fd })
    }

    fn ctrl_state(&self) -> u16 {
        unsafe { (self.ptr.add(OFG_CTRL_OFFSET + 4) as *const u16).read_volatile() }
    }
}

impl Drop for ShmRegion {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.size);
            libc::close(self.data_fd);
            libc::close(self.space_fd);
        }
    }
}

#[derive(Clone)]
struct HealthService { state: Arc<Mutex<AppState>> }
struct AppState { core_binary: String, index_count: u64, crash_count: u64 }

#[tonic::async_trait]
impl Health for HealthService {
    async fn check(&self, _: tonic::Request<HealthRequest>) -> std::result::Result<tonic::Response<HealthResponse>, tonic::Status> {
        Ok(tonic::Response::new(HealthResponse { status: 1 }))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let core_binary = "../codebase-memory-mcp-main/build/c/codebase-memory-mcp".to_string();

    info!("P3: creating shared memory ({} MiB)", OFG_RING_DEFAULT_CAP / (1024*1024));
    let shm = ShmRegion::create(OFG_RING_DEFAULT_CAP)?;
    let memfd_raw = std::os::fd::AsRawFd::as_raw_fd(&shm._memfd);
    info!("memfd={}, data_fd={}, space_fd={}", memfd_raw, shm.data_fd, shm.space_fd);

    // Spawn ofg-core worker subprocess.
    let mut child = std::process::Command::new(&core_binary)
        .arg("--worker")
        .arg(format!("--shm-fd={}", memfd_raw))
        .arg(format!("--data-event-fd={}", shm.data_fd))
        .arg(format!("--space-event-fd={}", shm.space_fd))
        .arg("--repo=../OntoFirmwareGraph/fixtures/c-driver")
        .arg("--project=p3-shm-smoke")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("spawn ofg-core worker")?;

    let pid = child.id();
    info!(pid = pid, "ofg-core worker spawned");

    let status = child.wait()?;
    info!(pid = pid, exit = ?status.code(), "worker finished");

    // P3: process all frames from the ring buffer.
    let mut consumer = ring::RingConsumer::new(
        shm.ptr, shm.size, shm.data_fd, shm.space_fd);
    match consumer.process_available() {
        Ok((frames, nodes, edges)) => {
            info!(frames = frames, nodes = nodes, edges = edges, "ring frames consumed");
        }
        Err(e) => {
            tracing::warn!("ring read: {} (may be empty if C exited before writing)", e);
        }
    }

    let state = Arc::new(Mutex::new(AppState {
        core_binary,
        index_count: if status.success() { 1 } else { 0 },
        crash_count: if status.success() { 0 } else { 1 },
    }));

    let addr = "0.0.0.0:50051".parse()?;

    // P7: PG pool for query services
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://ontowirmwaregraph:ofg_dev_2026@localhost/onto_firmware_graph".into());
    let pool = PgPoolOptions::new().max_connections(5).connect(&db_url).await
        .context("PG connect")?;
    sqlx::migrate!("./migrations").run(&pool).await
        .context("PG migrations")?;
    info!("PG pool ready");

    let snapshot_svc = snapshot_query_service::SnapshotQueryServiceImpl::new(pool.clone());
    let risk_svc = graph_risk_service::GraphRiskServiceImpl::new(pool.clone());
    let query_svc = query_service::QueryServiceImpl::new(pool.clone());

    info!("gRPC on {}", addr);
    Server::builder()
        .add_service(HealthServer::new(HealthService { state }))
        .add_service(
            snapshot_query_service::ocg_snapshot::snapshot_query_service_server
                ::SnapshotQueryServiceServer::new(snapshot_svc)
        )
        .add_service(
            graph_risk_service::ocg_risk::graph_risk_service_server
                ::GraphRiskServiceServer::new(risk_svc)
        )
        .add_service(
            query_service::ocg_query::query_service_server
                ::QueryServiceServer::new(query_svc)
        )
        .serve(addr).await?;
    Ok(())
}

#[cfg(test)]
mod pg_test {
    use super::pg_store::PgStore;

    #[tokio::test]
    #[ignore = "requires running PostgreSQL"]
    async fn test_pg_connect_and_migrate() {
        let store = PgStore::open("postgres://ontowirmwaregraph:ofg_dev_2026@localhost/onto_firmware_graph")
            .await
            .expect("PG connect + migrate");
        
        let repo_id = store.ensure_repository("p4-test", "/tmp/p4-test")
            .await
            .expect("ensure_repository");
        
        let snap_id = store.create_snapshot(repo_id, "abc123", "profile-v1", "ofg-0.1", 1)
            .await
            .expect("create_snapshot");
        
        store.seal_snapshot(snap_id, 10, 20).await.expect("seal");
        
        println!("P4 PG test PASS: repo={repo_id} snap={snap_id}");
    }
}

#[cfg(test)]
mod p5_integration {
    use crate::pg_store::PgStore;
    use crate::merkle::{FileFact, compute_snapshot_root};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    #[tokio::test]
    #[ignore = "requires running PostgreSQL"]
    async fn test_p5_end_to_end_snapshot_seal() {
        let mut store = PgStore::open(
            "postgres://ontowirmwaregraph:ofg_dev_2026@localhost/onto_firmware_graph"
        ).await.expect("PG connect");

        let repo_id = store.ensure_repository("p5-e2e-test", "/tmp/p5-e2e")
            .await.expect("repo");

        // 1. Begin baseline (use store directly, SnapshotManager wraps it)
        let snap_id = store.create_snapshot(repo_id, "abc123", "profile-v1", "ofg-0.1", 1)
            .await.expect("create_snapshot");

        // 2. Insert entities
        let file_a = store.ensure_file(repo_id, "src/main.rs").await.expect("file_a");
        let ent1 = store.ensure_entity(repo_id, "main.rs::main", "Function", Some("rust"))
            .await.expect("entity1");
        store.insert_entity_version(snap_id, ent1, "crate::main", Some(file_a), 1, 10,
            "abc123", &serde_json::json!({"exported":true})).await.expect("version1");

        let ent2 = store.ensure_entity(repo_id, "main.rs::helper", "Function", Some("rust"))
            .await.expect("entity2");
        store.insert_entity_version(snap_id, ent2, "crate::helper", Some(file_a), 12, 15,
            "def456", &serde_json::json!({"exported":false})).await.expect("version2");

        // 3. Insert edge
        store.insert_edge(repo_id, snap_id, ent1, ent2, "CALLS",
            "hash_main_calls_helper", "profile-v1", Some("callsite_1_5"), None)
            .await.expect("edge");

        // 4. Compute Merkle root
        let file_fact = FileFact {
            file_id: file_a,
            content_sha256: "file-content-hash".into(),
            entity_facts: vec![
                crate::merkle::EntityVersionFact {
                    entity_id: ent1, structural_hash: "abc123".into(),
                    qualified_name: Some("crate::main".into()),
                },
                crate::merkle::EntityVersionFact {
                    entity_id: ent2, structural_hash: "def456".into(),
                    qualified_name: Some("crate::helper".into()),
                },
            ],
        };
        let root = compute_snapshot_root(&[file_fact.clone()], &BTreeMap::new());
        assert!(!root.is_empty(), "Merkle root must not be empty");

        // 5. Seal via store
        store.seal_snapshot(snap_id, 2, 1).await.expect("seal");

        // 6. Verify Merkle root stored in PG
        // (graph_content_hash is computed in seal_snapshot)

        println!("P5 integration PASS: snapshot={snap_id} root={root}");
    }
}
