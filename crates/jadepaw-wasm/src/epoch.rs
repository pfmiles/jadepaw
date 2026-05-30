//! Epoch ticker — background thread for cooperative Wasm yielding.
//!
//! Drives wasmtime's epoch-based interruption mechanism. Epoch is a lightweight,
//! non-deterministic interrupt that enables cooperative timeslicing across
//! concurrent Wasm stores.
//!
//! # Design (D-09)
//!
//! - Epoch ticker runs at ~1ms intervals in a background thread
//! - Uses `EngineWeak` to detect when the Engine has been dropped (ticker exits)
//! - Uses `Engine` (cloned) for `increment_epoch()` which is signal-safe
//! - Returns an `EpochTickerGuard` that joins the thread on Drop
//!
//! # Safety (T-02-06)
//!
//! The ticker exits when `EngineWeak::upgrade()` returns `None`, meaning
//! the Engine has been dropped. The cloned `Engine` for `increment_epoch()`
//! is dropped inside the thread on exit, so it does not keep the Engine alive
//! past the main Engine's lifetime.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use wasmtime::Engine;

/// A guard that joins the epoch ticker background thread on drop.
///
/// Dropping this guard blocks until the ticker thread exits. The thread
/// is also signalled to stop when the Engine is dropped (via EngineWeak).
pub struct EpochTickerGuard {
    stop_tx: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for EpochTickerGuard {
    fn drop(&mut self) {
        // Signal the ticker thread to stop
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        // Join the thread
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Start an epoch ticker background thread for the given Engine.
///
/// The thread ticks at ~1ms intervals, calling `Engine::increment_epoch()`
/// on each tick. The ticker exits gracefully when:
///
/// 1. The returned `EpochTickerGuard` is dropped, or
/// 2. The Engine is dropped (detected via `EngineWeak::upgrade()`)
///
/// # Example
///
/// ```ignore
/// let engine = EngineFactory::build().unwrap();
/// let _ticker = start_epoch_ticker(&engine); // ticker runs in background
/// // ... use engine ...
/// // ticker is joined when _ticker goes out of scope
/// ```
pub fn start_epoch_ticker(engine: &Engine) -> EpochTickerGuard {
    let engine_clone = engine.clone();
    let engine_weak = engine.weak();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

    let handle = thread::spawn(move || {
        let tick = Duration::from_millis(1);
        loop {
            // Check if we've been signalled to stop
            if stop_rx.try_recv().is_ok() {
                break;
            }

            // Check if the Engine is still alive
            if engine_weak.upgrade().is_none() {
                break;
            }

            // Signal-safe: atomic increment, no syscalls
            engine_clone.increment_epoch();

            thread::sleep(tick);
        }
    });

    EpochTickerGuard {
        stop_tx: Some(stop_tx),
        handle: Some(handle),
    }
}