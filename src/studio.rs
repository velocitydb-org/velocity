use crate::addon::DatabaseManager;
use crate::{VeloError, VeloResult};
use axum::{
    response::Html,
    routing::{get, post},
    Json, Router,
};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;


pub struct StudioEngine {
    templates: HashMap<String, String>,
}

impl StudioEngine {
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    pub fn render(&self, template_name: &str, data: &HashMap<String, String>) -> String {
        let mut content = self
            .templates
            .get(template_name)
            .cloned()
            .unwrap_or_else(|| format!("Template {} not found", template_name));

        for (key, value) in data {
            let placeholder = format!("[[ {} ]]", key);
            content = content.replace(&placeholder, value);
        }

        content
    }

    pub fn register(&mut self, name: &str, content: &str) {
        self.templates.insert(name.to_string(), content.to_string());
    }
}

#[derive(Serialize)]
pub struct AnalysisReport {
    pub issues: Vec<String>,
    pub score: u8,
}

#[derive(Deserialize)]
struct ToggleAddonRequest {
    kind: String,
    enabled: bool,
}

#[derive(Deserialize)]
struct DatabaseLimitUpdateRequest {
    default_max_disk_size_bytes: Option<u64>,
}

pub fn analyze_system(config_path: &Path, db_manager: &DatabaseManager) -> AnalysisReport {
    let mut issues = Vec::new();
    let mut score = 100u8;


    if !config_path.exists() {
        issues.push("Configuration file (velocity.toml) is missing!".to_string());
        score = score.saturating_sub(50);
    } else if let Ok(content) = std::fs::read_to_string(config_path) {
        if !content.contains("bind_address") {
            issues.push("Security: No bind_address defined, using default.".to_string());
            score = score.saturating_sub(10);
        }
        if !content.contains("backup")
            || (content.contains("[addons.backup]") && content.contains("enabled = false"))
        {
            issues.push("Risk: Automatic backup system is disabled.".to_string());
            score = score.saturating_sub(20);
        }
    }


    let stats = db_manager.stats();
    if stats.sstable_count > 50 {
        issues.push(format!(
            "Performance: High SSTable count ({}), compaction might be lagging.",
            stats.sstable_count
        ));
        score = score.saturating_sub(15);
    }


    let db_config = db_manager.get_db_config();
    if !db_config.default_path.exists() {
        issues.push("Path Error: External database directory does not exist.".to_string());
        score = score.saturating_sub(10);
    }

    AnalysisReport { issues, score }
}

impl DatabaseManager {
    pub fn get_db_config(&self) -> crate::addon::DatabaseAddonConfig {
        self.db_config.read().unwrap().clone()
    }
}

pub async fn start_studio(
    addr: SocketAddr,
    db_manager: Arc<DatabaseManager>,
    config_path: PathBuf,
) -> VeloResult<()> {
    let mut engine = StudioEngine::new();
    engine.register("index", get_studio_html());
    let engine = Arc::new(engine);

    let app = Router::new()
        .route(
            "/",
            get({
                let engine = engine.clone();
                move || async move {
                    let mut data = HashMap::new();
                    data.insert("VERSION".to_string(), "0.2.0".to_string());
                    Html(engine.render("index", &data))
                }
            }),
        )
        .route(
            "/api/analysis",
            get({
                let manager = db_manager.clone();
                let cfg = config_path.clone();
                move || async move {
                    let report = analyze_system(&cfg, &manager);
                    Json(report)
                }
            }),
        )
        .route(
            "/api/stats",
            get({
                let manager = db_manager.clone();
                move || async move { Json(manager.stats()) }
            }),
        )
        .route(
            "/api/addons",
            get({
                let manager = db_manager.clone();
                move || async move {
                    let addons = manager.list_addons();
                    let response: Vec<HashMap<String, String>> = addons
                        .into_iter()
                        .map(|(name, enabled)| {
                            let mut map = HashMap::new();
                            map.insert("name".to_string(), name);
                            map.insert("enabled".to_string(), enabled.to_string());
                            map
                        })
                        .collect();
                    Json(response)
                }
            }),
        )
        .route(
            "/api/addons/toggle",
            post({
                let manager = db_manager.clone();
                move |headers: axum::http::HeaderMap, Json(payload): Json<ToggleAddonRequest>| async move {

                    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("");
                    if !host.starts_with("localhost") && !host.starts_with("127.0.0.1") {
                        return Json(serde_json::json!({ "status": "error", "message": "Access Denied" }));
                    }

                    let kind = match payload.kind.as_str() {
                        "database" => crate::addon::AddonKind::Database,
                        "backup" => crate::addon::AddonKind::Backup,
                        "background-service" | "background_service" => crate::addon::AddonKind::BackgroundService,
                        _ => return Json(serde_json::json!({ "status": "error", "message": "Unknown addon" })),
                    };

                    if let Err(e) = manager.toggle_addon(kind, payload.enabled) {
                        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
                    }

                    Json(serde_json::json!({ "status": "ok" }))
                }
            }),
        )
        .route(
            "/api/database/limits",
            get({
                let manager = db_manager.clone();
                move || async move {
                    Json(serde_json::json!({
                        "default_max_disk_size_bytes": manager.get_default_database_max_disk_size_bytes()
                    }))
                }
            }),
        )
        .route(
            "/api/database/limits",
            post({
                let manager = db_manager.clone();
                move |headers: axum::http::HeaderMap, Json(payload): Json<DatabaseLimitUpdateRequest>| async move {
                    let host = headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or("");
                    if !host.starts_with("localhost") && !host.starts_with("127.0.0.1") {
                        return Json(serde_json::json!({ "status": "error", "message": "Access Denied" }));
                    }

                    if let Err(e) = manager.set_default_database_max_disk_size_bytes(payload.default_max_disk_size_bytes) {
                        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
                    }

                    Json(serde_json::json!({ "status": "ok" }))
                }
            }),
        )
        .layer(tower_http::cors::CorsLayer::permissive());

    println!(
        "{} Velocity Studio starting at http://{}...",
        "[STUDIO]".cyan().bold(),
        addr
    );

    let url = format!("http://{}", addr);
    let _ = open::that(&url);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .await
        .map_err(|e| VeloError::InvalidOperation(e.to_string()))?;

    Ok(())
}

fn get_studio_html() -> &'static str {
    include_str!("studio_index.html")
}
