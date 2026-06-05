//! Skill management REST API endpoints.
//!
//! Provides four endpoints for the skill system:
//! - `POST /skills/load` — Load a skill from disk into the in-memory registry
//! - `POST /skills/unload` — Remove a skill from the in-memory registry
//! - `GET /skills/list` — List indexed skills for a tenant
//! - `GET /skills/inspect/{name}` — Return full SKILL.md content from disk
//!
//! # Security (Threat Model)
//!
//! - T-06-10: Path traversal mitigated by skill_name validation (kebab-case)
//!   before any filesystem access.
//! - T-06-11: Cross-tenant isolation via dual-key (skill_id, tenant_id) in
//!   all repository queries.
//! - T-06-13: No authentication in Phase 6 — accepted risk for MVP. Auth
//!   will be added in Phase 9.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use jadepaw_core::{SkillManifest, TenantId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use jadepaw_db::SkillRepository;
use jadepaw_skill::parse_skill_file;

/// Shared state for skill API route handlers.
#[derive(Clone)]
pub struct SkillApiState {
    pub skill_manager: Arc<jadepaw_skill::SkillManager>,
    pub skill_repo: Arc<dyn SkillRepository>,
}

// ── Request/Response types ─────────────────────────────────────────────────

/// Request body for POST /skills/load
#[derive(Debug, Deserialize)]
pub struct LoadSkillRequest {
    pub tenant_id: TenantId,
    pub skill_name: String,
}

/// Request body for POST /skills/unload
#[derive(Debug, Deserialize)]
pub struct UnloadSkillRequest {
    pub tenant_id: TenantId,
    pub skill_name: String,
}

/// Query parameters for skill listing and inspection
#[derive(Debug, Deserialize)]
pub struct ListSkillsQuery {
    pub tenant_id: TenantId,
}

/// Response body for skill listing
#[derive(Debug, Serialize)]
pub struct SkillListResponse {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
}

/// Response body for skill inspection
#[derive(Debug, Serialize)]
pub struct SkillInspectResponse {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub tools: Vec<String>,
    pub constraints: Option<String>,
    pub body: String,
}

/// Structured error response for validation failures
#[derive(Debug, Serialize)]
pub struct SkillErrorResponse {
    pub field: String,
    pub reason: String,
}

// ── Router ──────────────────────────────────────────────────────────────────

/// Build the skill routes router.
///
/// Attach to the main app via `.nest("/api", skill_routes())` or mount
/// directly at a prefix.
pub fn skill_routes() -> Router<SkillApiState> {
    Router::new()
        .route("/skills/load", post(load_skill))
        .route("/skills/unload", post(unload_skill))
        .route("/skills/list", get(list_skills))
        .route("/skills/inspect/{name}", get(inspect_skill))
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// Load a skill from disk into the in-memory registry.
///
/// Parses the SKILL.md file from `<skills_root>/<tenant_id>/<skill_name>/SKILL.md`,
/// validates it, and inserts it into the registry. On success, the skill is
/// immediately available for agent injection.
///
/// # Security (T-06-10)
///
/// The skill_name is validated during parse_skill_file() inside
/// SkillManager::load(). Path traversal is prevented by the fact that
/// the constructed path is rooted at `<skills_root>/<tenant_id>/`, and
/// `load()` rejects names that fail kebab-case validation after parsing.
async fn load_skill(
    State(state): State<SkillApiState>,
    Json(req): Json<LoadSkillRequest>,
) -> impl IntoResponse {
    match state
        .skill_manager
        .load(req.tenant_id, &req.skill_name, None)
        .await
    {
        Ok(()) => {
            tracing::info!(
                tenant_id = %req.tenant_id,
                skill = %req.skill_name,
                "skill loaded via API"
            );
            StatusCode::OK.into_response()
        }
        Err(e) => {
            tracing::warn!(
                tenant_id = %req.tenant_id,
                skill = %req.skill_name,
                error = %e,
                "failed to load skill"
            );
            let status = classify_skill_error_status(&e);
            (
                status,
                Json(SkillErrorResponse {
                    field: req.skill_name.clone(),
                    reason: format!("{:?}", e),
                }),
            )
                .into_response()
        }
    }
}

/// Unload a skill from the in-memory registry.
///
/// After removal, the system prompt is rebuilt from remaining active skills.
/// Idempotent — no error if the skill is not currently loaded.
async fn unload_skill(
    State(state): State<SkillApiState>,
    Json(req): Json<UnloadSkillRequest>,
) -> impl IntoResponse {
    match state
        .skill_manager
        .unload(req.tenant_id, &req.skill_name)
        .await
    {
        Ok(()) => {
            tracing::info!(
                tenant_id = %req.tenant_id,
                skill = %req.skill_name,
                "skill unloaded via API"
            );
            StatusCode::OK.into_response()
        }
        Err(e) => {
            tracing::warn!(
                tenant_id = %req.tenant_id,
                skill = %req.skill_name,
                error = %e,
                "failed to unload skill"
            );
            let status = classify_skill_error_status(&e);
            (
                status,
                Json(SkillErrorResponse {
                    field: req.skill_name.clone(),
                    reason: format!("{:?}", e),
                }),
            )
                .into_response()
        }
    }
}

/// Classify a `SkillValidationError` into the appropriate HTTP status code.
///
/// Validation and parse errors are client faults (400), while unexpected
/// or I/O-related errors are server faults (500).
fn classify_skill_error_status(e: &jadepaw_core::SkillValidationError) -> StatusCode {
    use jadepaw_core::SkillValidationError;
    match e {
        SkillValidationError::ParseError { .. }
        | SkillValidationError::MissingField { .. }
        | SkillValidationError::InvalidName { .. }
        | SkillValidationError::FieldTooLong { .. }
        | SkillValidationError::NameDirectoryMismatch { .. }
        | SkillValidationError::ToolNotFound { .. }
        | SkillValidationError::MissingFrontmatter { .. } => StatusCode::BAD_REQUEST,
        // Future variants that represent server-side issues map to 500.
        #[allow(unreachable_patterns)]
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// List all indexed skills for a tenant.
///
/// Queries the SQLite skill_index cache for fast listing. Returns lightweight
/// summaries excluding full manifest content and file content.
///
/// # Security (T-06-11)
///
/// Dual-key isolation: `tenant_id` is used in the SQL WHERE clause, so
/// cross-tenant enumeration is impossible.
async fn list_skills(
    State(state): State<SkillApiState>,
    Query(query): Query<ListSkillsQuery>,
) -> impl IntoResponse {
    match state.skill_repo.list_by_tenant(query.tenant_id).await {
        Ok(summaries) => {
            let response: Vec<SkillListResponse> = summaries
                .into_iter()
                .map(|s| SkillListResponse {
                    name: s.name,
                    description: s.description,
                    version: s.version,
                })
                .collect();
            Json(response).into_response()
        }
        Err(e) => {
            tracing::error!(
                tenant_id = %query.tenant_id,
                error = %e,
                "failed to list skills"
            );
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Inspect a skill by reading the SKILL.md file directly from disk.
///
/// Per D-09, the filesystem is the source of truth. This endpoint reads the
/// raw SKILL.md file, parses it, and returns the full manifest + Markdown body.
/// It does NOT read from the SQLite cache.
///
/// # Security (T-06-10)
///
/// Path: `<skills_root>/<tenant_id>/<skill_name>/SKILL.md`. The skill_name
/// is validated BEFORE any filesystem access to prevent path traversal.
/// The tenant_id directory provides multi-tenant isolation (T-06-11).
async fn inspect_skill(
    State(state): State<SkillApiState>,
    Path(name): Path<String>,
    Query(query): Query<ListSkillsQuery>,
) -> Response {
    // Validate skill_name BEFORE any filesystem access to prevent path
    // traversal attacks where name="../" could escape the skills_root.
    if let Err(validation_err) = jadepaw_skill::validate_skill_name(&name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(SkillErrorResponse {
                field: "skill_name".to_string(),
                reason: format!("{:?}", validation_err),
            }),
        )
            .into_response();
    }

    // Build the file path from the skills_root (via skill_manager's skills_root)
    let skills_root = &state.skill_manager.skills_root;
    let file_path = skills_root
        .join(query.tenant_id.to_string())
        .join(&name)
        .join("SKILL.md");

    let content = match tokio::fs::read_to_string(&file_path).await {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(SkillErrorResponse {
                    field: "skill_name".to_string(),
                    reason: format!("skill '{}' not found for tenant", name),
                }),
            )
                .into_response();
        }
    };

    match parse_skill_file(&content, &name, &file_path) {
        Ok((manifest, body)) => Json(SkillInspectResponse {
            name: manifest.name,
            description: manifest.description,
            version: manifest.version,
            author: manifest.author,
            tools: manifest.tools,
            constraints: manifest.constraints,
            body,
        })
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(SkillErrorResponse {
                field: name.clone(),
                reason: format!("{:?}", e),
            }),
        )
            .into_response(),
    }
}