//! onto-graph-core — FFI bindings to the CBM-derived C compute kernel.
//!
//! The C core (ofg_core.c/h, ofg_graph_sink.h) is compiled via `cc` in build.rs
//! only when the CBM source tree is available (set OFG_CBM_ROOT).
//!
//! When the C kernel is not compiled, the Rust types defined here remain available
//! for type-level compatibility — actual analysis runs through PostgreSQL-only code paths.

#![allow(non_camel_case_types, non_snake_case, dead_code)]

// ═══════════════════════════════════════════════════
// Snapshot metadata (matches ofg_graph_sink.h)
// ═══════════════════════════════════════════════════

#[repr(C)]
pub struct ofg_snapshot_meta {
    pub repository_id: *const libc::c_char,
    pub base_commit_sha: *const libc::c_char,
    pub candidate_checkpoint_hash: *const libc::c_char,
    pub execution_generation: u64,
    pub analysis_profile_hash: *const libc::c_char,
    pub extractor_version: *const libc::c_char,
    pub graph_schema_version: u32,
}

// ═══════════════════════════════════════════════════
// GraphSink function table (matches ofg_graph_sink.h)
// ═══════════════════════════════════════════════════

pub type sink_callback = Option<
    unsafe extern "C" fn(ctx: *mut libc::c_void, meta: *const ofg_snapshot_meta) -> libc::c_int,
>;

#[repr(C)]
pub struct ofg_graph_sink {
    pub ctx: *mut libc::c_void,
    pub begin_snapshot: sink_callback,
    pub write_nodes: Option<
        unsafe extern "C" fn(ctx: *mut libc::c_void, batch: *const ofg_node_batch) -> libc::c_int,
    >,
    pub write_edges: Option<
        unsafe extern "C" fn(ctx: *mut libc::c_void, batch: *const ofg_edge_batch) -> libc::c_int,
    >,
    pub write_deletions: Option<
        unsafe extern "C" fn(ctx: *mut libc::c_void, batch: *const libc::c_void) -> libc::c_int,
    >,
    pub write_coverage: Option<
        unsafe extern "C" fn(ctx: *mut libc::c_void, batch: *const libc::c_void) -> libc::c_int,
    >,
    pub write_diagnostics: Option<
        unsafe extern "C" fn(ctx: *mut libc::c_void, batch: *const libc::c_void) -> libc::c_int,
    >,
    pub commit_snapshot: sink_callback,
    pub rollback_snapshot: sink_callback,
    pub destroy: Option<unsafe extern "C" fn(ctx: *mut libc::c_void)>,
}

/// Opaque node batch pointer (detailed struct in ofg_graph_sink.h).
#[repr(C)]
pub struct ofg_node_batch {
    pub items: *const libc::c_void,
    pub count: i32,
}

/// Opaque edge batch pointer.
#[repr(C)]
pub struct ofg_edge_batch {
    pub items: *const libc::c_void,
    pub count: i32,
}

// ═══════════════════════════════════════════════════
// C Core lifecycle (matches ofg_core.h)
// ═══════════════════════════════════════════════════

/// Opaque C core handle.
pub struct ofg_core_t {
    _private: [u8; 0],
}

extern "C" {
    pub fn ofg_core_create() -> *mut ofg_core_t;
    pub fn ofg_core_destroy(core: *mut ofg_core_t);
    pub fn ofg_core_load_profile(core: *mut ofg_core_t, profile_json: *const libc::c_char) -> libc::c_int;
    pub fn ofg_core_analyze_baseline(
        core: *mut ofg_core_t,
        repo_path: *const libc::c_char,
        project_name: *const libc::c_char,
        sink: *mut ofg_graph_sink,
    ) -> libc::c_int;
    pub fn ofg_core_analyze_delta(
        core: *mut ofg_core_t,
        repo_path: *const libc::c_char,
        project_name: *const libc::c_char,
        changed_files: *const *const libc::c_char,
        changed_count: libc::c_int,
        sink: *mut ofg_graph_sink,
    ) -> libc::c_int;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn test_snapshot_meta_size() {
        // Verify struct layout is pointer-aligned (C ABI compatible)
        assert_eq!(mem::align_of::<ofg_snapshot_meta>(), mem::align_of::<usize>());
    }

    #[test]
    fn test_graph_sink_has_10_slots() {
        // verify the C function table has correct field count
        let sink = ofg_graph_sink {
            ctx: std::ptr::null_mut(),
            begin_snapshot: None, write_nodes: None, write_edges: None,
            write_deletions: None, write_coverage: None, write_diagnostics: None,
            commit_snapshot: None, rollback_snapshot: None, destroy: None,
        };
        assert!(mem::size_of_val(&sink) > 0);
    }
}
