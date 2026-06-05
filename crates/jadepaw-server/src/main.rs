//! jadepaw server entry point.
//!
//! Startup sequence:
//! 1. Determine skills root directory (~/.jadepaw/skills/)
//! 2. Initialize SQLite database pool with WAL mode
//! 3. Run database migrations (sessions + skill_index tables)
//! 4. Perform walkdir startup scan to discover SKILL.md files
//! 5. Sync discovered skills to SQLite skill_index cache
//! 6. Build SkillManager and shared API state
//! 7. Start axum HTTP server on 127.0.0.1:3000

mod routes;

use anyhow::Context;
use std::sync::Arc;

use axum::Router;
use jadepaw_db::{SqliteSkillRepo, SkillRepository};
use jadepaw_skill::{SkillIndex, SkillLoader, SkillManager};
use routes::{skill_routes, SkillApiState};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::str::FromStr;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // ── 1. Determine skills root directory ──────────────────────────────
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    let jadepaw_home = std::path::PathBuf::from(&home).join(".jadepaw");
    let skills_root = jadepaw_home.join("skills");
    tokio::fs::create_dir_all(&skills_root)
        .await
        .context("failed to create skills directory")?;

    // ── 2. Initialize database pool ─────────────────────────────────────
    let db_path = jadepaw_home.join("jadepaw.db");
    let db_path_str = db_path.to_string_lossy().to_string();

    let opts = SqliteConnectOptions::from_str(&db_path_str)
        .context("invalid database path")?
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true)
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .context("failed to create SQLite connection pool")?;

    // Run migrations (includes both sessions and skill_index tables)
    sqlx::migrate!("../jadepaw-db/migrations")
        .run(&pool)
        .await
        .context("failed to run database migrations")?;

    tracing::info!("database initialized and migrations complete");

    // ── 3. Startup walkdir scan + SQLite index sync (D-09) ──────────────
    let skill_repo: Arc<dyn SkillRepository> =
        Arc::new(SqliteSkillRepo::new(pool.clone()));
    let skill_index = SkillIndex::new(skill_repo.clone());

    let scan_entries = {
        let loader_clone = skills_root.clone(); // used in blocking task
        tokio::task::spawn_blocking(move || {
            let loader = SkillLoader::new(loader_clone);
            loader.scan_all()
        })
    }
    .await
    .context("skill scan panicked")?;

    tracing::info!(count = scan_entries.len(), "startup skill scan complete");

    skill_index
        .sync(&scan_entries)
        .await
        .context("failed to sync skill index")?;

    tracing::info!("skill index synced");

    // ── 4. Build SkillManager and shared app state ──────────────────────
    let skill_manager = Arc::new(SkillManager::new(skills_root));

    let api_state = SkillApiState {
        skill_manager,
        skill_repo,
    };

    // ── 5. Build axum router ────────────────────────────────────────────
    let app = Router::new()
        .nest("/api", skill_routes())
        .with_state(api_state);

    // ── 6. Start server ─────────────────────────────────────────────────
    let bind_addr = "127.0.0.1:3000";
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .context(format!("failed to bind to {}", bind_addr))?;

    tracing::info!("jadepaw server listening on http://{}", bind_addr);

    axum::serve(listener, app)
        .await
        .context("server error")?;

    Ok(())
}