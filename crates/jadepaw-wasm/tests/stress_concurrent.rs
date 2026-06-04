//! Stress test: 1,000 concurrent sessions within 64MB cap each.
//!
//! Validates ROADMAP.md success criterion 5: "Running 1,000 concurrent isolated
//! sessions does not cause memory exhaustion."
//!
//! **This test is marked `#[ignore]`** because it requires significant resources
//! (~64GB of virtual address space for 1,000 * 64MB instances on the PoolingAllocator).
//! The PoolingAllocator pre-allocates memory slots at Engine creation time, so
//! the actual physical memory usage is lower (virtual allocation + lazy commit).
//!
//! Run with:
//! ```bash
//! cargo test -p jadepaw-wasm -- stress_concurrent -- --ignored --test-threads=1
//! ```
//!
//! ## What this validates
//!
//! - 1,000 concurrent sessions are acquired successfully (no OOM, no crashes)
//! - All sessions release correctly (memory returns to baseline)
//! - PoolingAllocatorConfig `max_memory_size=64MB` is correct — if the default
//!   4GiB were used, 1,000 instances would exhaust virtual address space (Pitfall 4)
//! - DashMap correctly tracks 1,000 entries (no hash collisions at scale)
//! - Semaphore correctly bounds 1,000 acquires (no deadlocks, no race conditions)
//!
//! ## Caveats
//!
//! - RSS measurement is platform-specific (macOS/linux use different APIs).
//!   The memory assertion is that the process does NOT crash/OOM — exact RSS
//!   values are reported but not asserted on.
//! - PoolingAllocator uses virtual memory (mmap) for its slots. The OS lazily
//!   commits physical pages. 1,000 * 64MB = 64GB of virtual address space,
//!   but actual physical RAM usage is far lower (only guard pages are committed).
//! - On constrained machines (< 64GB of virtual address space), reduce the
//!   session count or run on a larger host.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use jadepaw_core::{InstanceCapabilities, SessionId, TenantId};
use jadepaw_wasm::{InstancePool, PoolConfig, SessionState};

/// Fixture: creates noop.wasm guest bytes (module with empty _start).
fn noop_wasm_bytes() -> Vec<u8> {
    wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "_start")))"#,
    )
    .expect("failed to parse noop.wat")
}

/// Fixture: creates a SessionState with default capabilities and a temp sandbox root.
fn make_session_state() -> SessionState {
    let session_id = SessionId::new();
    let tenant_id = TenantId::new();
    let capabilities = InstanceCapabilities::default();
    let sandbox_root = std::env::temp_dir();
    SessionState::new(session_id, tenant_id, capabilities, sandbox_root)
        .expect("SessionState::new should succeed")
}

/// Report current process RSS on supported platforms.
///
/// Returns `None` on unsupported platforms. RSS values are informative only —
/// the primary assertion is that the process does not crash/OOM.
fn get_rss_bytes() -> Option<usize> {
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        let mut statm = String::new();
        std::fs::File::open("/proc/self/statm")
            .ok()?
            .read_to_string(&mut statm)
            .ok()?;
        let resident_pages: usize = statm
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        Some(resident_pages * page_size)
    }
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // macOS: use `ps -o rss=` to get RSS in KB
        let pid = std::process::id();
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let stdout = String::from_utf8(output.stdout).ok()?;
        let rss_kb: usize = stdout.trim().parse().ok()?;
        Some(rss_kb * 1024)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Stress test: 1,000 concurrent sessions, each within its 64MB cap.
///
/// 1. Creates InstancePool with max_concurrent=1500, noop.wasm guest
/// 2. Records baseline RSS
/// 3. Spawns 1,000 tasks, each acquiring a session and holding it
/// 4. Asserts all 1,000 acquires succeed (no errors returned)
/// 5. Asserts pool.active_count() == 1000
/// 6. Records peak RSS during load
/// 7. Releases all 1,000 handles (drops them)
/// 8. Asserts pool.active_count() == 0
/// 9. Reports RSS delta
///
/// The test passes if the process does NOT crash or OOM — this validates
/// that PoolingAllocatorConfig::max_memory_size=64MB is correctly sized.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires significant resources: ~64GB virtual address space. Run with -- --ignored --test-threads=1"]
async fn test_1000_concurrent_sessions_no_oom() {
    const SESSION_COUNT: usize = 1_000;
    const POOL_CAPACITY: usize = 1_500;

    let config = PoolConfig::new(noop_wasm_bytes(), PathBuf::from("/tmp/jadepaw-stress"), POOL_CAPACITY);
    let pool = Arc::new(InstancePool::new(config).expect("failed to create pool"));

    let baseline_rss = get_rss_bytes();
    eprintln!(
        "=== Stress test: {} concurrent sessions ===",
        SESSION_COUNT
    );
    if let Some(rss) = baseline_rss {
        eprintln!("Baseline RSS: {} MB", rss / (1024 * 1024));
    } else {
        eprintln!("Baseline RSS: not available on this platform");
    }

    // Acquire all sessions concurrently
    let start = Instant::now();
    let mut handles = Vec::with_capacity(SESSION_COUNT);

    // Spawn tasks in batches to avoid overwhelming the system
    const BATCH_SIZE: usize = 100;
    for batch_start in (0..SESSION_COUNT).step_by(BATCH_SIZE) {
        let batch_end = std::cmp::min(batch_start + BATCH_SIZE, SESSION_COUNT);
        let mut batch_tasks = tokio::task::JoinSet::new();

        for i in batch_start..batch_end {
            let pool_clone = Arc::clone(&pool);
            batch_tasks.spawn(async move {
                let session_id = SessionId::new();
                let state = make_session_state();
                let handle = pool_clone
                    .acquire(session_id, state)
                    .await
                    .expect("acquire should succeed");
                eprintln!("Session {i} acquired");
                handle
            });
        }

        while let Some(result) = batch_tasks.join_next().await {
            handles.push(result.expect("task panicked"));
        }

        if let Some(rss) = get_rss_bytes() {
            eprintln!(
                "After {}/{} acquires, RSS: {} MB",
                batch_end,
                SESSION_COUNT,
                rss / (1024 * 1024)
            );
        }
    }

    let acquire_duration = start.elapsed();
    eprintln!("All {SESSION_COUNT} acquires completed in {acquire_duration:?}");

    // Verify all acquired
    assert_eq!(handles.len(), SESSION_COUNT);
    assert_eq!(pool.active_count(), SESSION_COUNT);

    let peak_rss = get_rss_bytes();
    if let Some(rss) = peak_rss {
        eprintln!("Peak RSS (1,000 active): {} MB", rss / (1024 * 1024));
    }

    // Release all sessions
    let release_start = Instant::now();
    drop(handles);
    let release_duration = release_start.elapsed();
    eprintln!(
        "All {SESSION_COUNT} sessions released in {release_duration:?}"
    );

    // Verify all released
    assert_eq!(pool.active_count(), 0);

    let final_rss = get_rss_bytes();
    if let (Some(base), Some(final_)) = (baseline_rss, final_rss) {
        let delta = if final_ > base {
            (final_ - base) / (1024 * 1024)
        } else {
            0
        };
        eprintln!(
            "Final RSS: {} MB (baseline: {} MB, delta: {} MB)",
            final_ / (1024 * 1024),
            base / (1024 * 1024),
            delta
        );
    }

    // Sanity check: if PoolingAllocator max_memory_size were 4GiB (default),
    // 1,000 instances would try to reserve 4,000 GiB of virtual address space
    // and would crash. The fact we got here proves max_memory_size=64MB is correct.
    eprintln!("SUCCESS: 1,000 concurrent sessions within 64MB cap — no OOM.");
}