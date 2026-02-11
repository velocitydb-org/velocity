use crate::config::ConfigFile;
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;
use velocity::server::{ServerConfig, VelocityServer};
use velocity::{Velocity, VelocityConfig};
use colored::*;

pub struct ServiceSpec {
    pub config_path: PathBuf,
    pub data_dir: PathBuf,
    pub bind: Option<String>,
    pub verbose: bool,
    pub pid_file: Option<PathBuf>,
    pub watch_config: bool,
}

struct PidGuard {
    path: PathBuf,
}

impl PidGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub async fn run_velocity_service(spec: ServiceSpec) -> Result<(), Box<dyn std::error::Error>> {
    let ServiceSpec {
        config_path,
        data_dir,
        bind,
        verbose,
        pid_file,
        watch_config,
    } = spec;

    if !data_dir.exists() {
        fs::create_dir_all(&data_dir)?;
    }

    let file_config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        toml::from_str::<ConfigFile>(&content)?
    } else {
        println!(
            "{} Config file not found, creating default...",
            "[WARN]".yellow()
        );
        let default_cfg = ConfigFile::default();
        let toml_string = toml::to_string_pretty(&default_cfg)?;
        fs::write(&config_path, toml_string)?;
        default_cfg
    };

    let log_level = if verbose {
        "debug"
    } else {
        &file_config.logging.level
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    println!(
        "{} Loading configuration from {:?}",
        "[CONFIG]".blue(),
        config_path
    );

    let mut server_config = ServerConfig::default();
    let bind_addr = bind.unwrap_or_else(|| file_config.server.bind_address.clone());
    server_config.bind_address = bind_addr.parse()?;
    server_config.max_connections = file_config.server.max_connections;
    server_config.connection_timeout =
        Duration::from_secs(file_config.server.connection_timeout_seconds);
    server_config.rate_limit_per_second = file_config.server.rate_limit_per_second;
    server_config.users = file_config.users.clone();
    server_config.audit_log_path = file_config.security.audit_log_path.clone();
    server_config.audit_logging = file_config.security.audit_logging;
    server_config.auth_ban_duration =
        Duration::from_secs(file_config.security.auth_ban_duration);
    server_config.max_auth_failures = file_config.security.max_auth_failures;

    let db_config = VelocityConfig {
        max_memtable_size: file_config.database.max_memtable_size,
        cache_size: file_config.database.cache_size,
        bloom_false_positive_rate: file_config.database.bloom_false_positive_rate,
        compaction_threshold: file_config.database.compaction_threshold,
        enable_compression: file_config.database.enable_compression,
        memory_only_mode: false,
        batch_wal_writes: true,
        adaptive_cache: file_config.performance.adaptive_cache,
        enable_metrics: file_config.performance.enable_metrics,
        metrics_interval: Duration::from_secs(file_config.performance.metrics_interval),
        target_cache_hit_rate: file_config.performance.target_cache_hit_rate,
        wal_sync_mode: velocity::WalSyncMode::Batch,
    };

    println!(
        "{} Initializing storage at {:?}",
        "[STORAGE]".blue(),
        data_dir
    );
    let db = Velocity::open_with_config(&data_dir, db_config)?;
    let db_manager = Arc::new(velocity::addon::DatabaseManager::new(db, config_path.clone()));

    let server = VelocityServer::new(db_manager.clone(), server_config)?;

    let _pid_guard = pid_file.as_ref().map(|path| {
        let pid = std::process::id().to_string();
        let _ = fs::write(path, pid);
        PidGuard::new(path.clone())
    });

    let _config_watcher = if watch_config {
        Some(setup_config_watcher(&config_path, &db_manager)?)
    } else {
        None
    };

    if let Some(backup_cfg) = &file_config.addons.backup {
        if backup_cfg.enabled {
            spawn_backup_loop(db_manager.clone(), backup_cfg.interval_minutes);
        }
    }

    println!(
        "{} Velocity service starting on {}...",
        "[SERVER]".green(),
        bind_addr.bold()
    );

    let mut server_future = server.start();
    tokio::pin!(server_future);

    tokio::select! {
        res = &mut server_future => res?,
        _ = tokio::signal::ctrl_c() => {
            log::info!("Shutdown signal received");
        }
    }

    Ok(())
}

fn spawn_backup_loop(manager: Arc<velocity::addon::DatabaseManager>, interval: u64) {
    println!(
        "{} Automatic backups enabled ({} min interval)",
        "[INFO]".green(),
        interval
    );

    tokio::spawn(async move {
        let mut interval_timer = tokio::time::interval(Duration::from_secs(interval * 60));
        interval_timer.tick().await;

        loop {
            interval_timer.tick().await;

            let current_enabled = manager
                .list_addons()
                .iter()
                .find(|(name, _)| name == "backup")
                .map(|(_, enabled)| *enabled)
                .unwrap_or(false);

            if current_enabled {
                match manager.backup_all_databases() {
                    Ok(succ) => log::info!(
                        "Automatic backup successful for {} databases",
                        succ.len()
                    ),
                    Err(e) => log::error!("Automatic backup failed: {}", e),
                }
            }
        }
    });
}

fn setup_config_watcher(
    config: &Path,
    manager: &Arc<velocity::addon::DatabaseManager>,
) -> notify::Result<RecommendedWatcher> {
    let manager_for_watcher = manager.clone();
    let (tx, mut rx) = mpsc::channel(1);

    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if event.kind.is_modify() {
                    let _ = tx.clone().try_send(());
                }
            }
        },
        NotifyConfig::default(),
    )?;

    watcher.watch(config, RecursiveMode::NonRecursive)?;

    tokio::spawn(async move {
        while let Some(_) = rx.recv().await {
            println!(
                "{} Configuration change detected, reloading...",
                "[CONFIG]".blue()
            );
            let _ = manager_for_watcher.reload_config();
        }
    });

    Ok(watcher)
}
