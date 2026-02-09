use velocity::VeloError;
use clap::{Parser, Subcommand};
use colored::*;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Password, Select};
use env_logger;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio;
use velocity::addon::BackupAddonConfig;

use velocity::addon::DatabaseAddonConfig;
use velocity::server::{hash_password, ServerConfig, VelocityServer};
use velocity::{Velocity, VelocityConfig};

#[derive(Parser)]
#[command(name = "velocitydb")]
#[command(version, about = "VelocityDB - Modern, High-performance Database")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {

    Server {

        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,


        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,


        #[arg(short, long)]
        bind: Option<String>,


        #[arg(short, long)]
        verbose: bool,
    },


    CreateUser {

        #[arg(short, long)]
        username: Option<String>,


        #[arg(short, long)]
        password: Option<String>,


        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
    },


    Init {

        #[arg(short, long, default_value = "velocity.toml")]
        output: PathBuf,
    },


    Addon {
        #[command(subcommand)]
        subcommand: AddonCommands,
    },


    Backup {

        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,


        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
    },


    Benchmark {

        #[arg(short, long, default_value = "./benchmark_db")]
        data_dir: PathBuf,


        #[arg(short, long, default_value = "100000")]
        operations: usize,


        #[arg(short, long, default_value = "standard")]
        mode: String,


        #[arg(long)]
        cache_size: Option<usize>,
    },


    Studio {

        #[arg(short, long, default_value = "3000")]
        port: u16,


        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,


        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
    },


    Monitor {

        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,


        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum AddonCommands {

    List {

        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
    },

    Enable {

        name: String,

        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
    },

    Disable {

        name: String,

        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ConfigFile {
    server: ServerConfigSection,
    #[serde(default)]
    logging: LoggingSection,
    #[serde(default)]
    performance: PerformanceSection,
    #[serde(default)]
    security: SecuritySection,
    users: HashMap<String, String>,
    database: DatabaseConfigSection,
    #[serde(default)]
    addons: AddonsSection,
}

#[derive(Debug, Serialize, Deserialize)]
struct ServerConfigSection {
    #[serde(default = "default_bind_address")]
    bind_address: String,
    #[serde(default = "default_max_connections")]
    max_connections: usize,
    #[serde(default = "default_timeout")]
    connection_timeout_seconds: u64,
    #[serde(default = "default_rate_limit")]
    rate_limit_per_second: u32,
    #[serde(default)]
    enable_tls: bool,
}

fn default_bind_address() -> String {
    "127.0.0.1:2005".to_string()
}
fn default_max_connections() -> usize {
    1000
}
fn default_timeout() -> u64 {
    300
}
fn default_rate_limit() -> u32 {
    1000
}

impl Default for ServerConfigSection {
    fn default() -> Self {
        Self {
            bind_address: default_bind_address(),
            max_connections: default_max_connections(),
            connection_timeout_seconds: default_timeout(),
            rate_limit_per_second: default_rate_limit(),
            enable_tls: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct LoggingSection {
    #[serde(default = "default_log_level")]
    level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PerformanceSection {
    #[serde(default = "default_bool_true")]
    adaptive_cache: bool,
    #[serde(default = "default_bool_true")]
    enable_metrics: bool,
    #[serde(default = "default_metrics_interval")]
    metrics_interval: u64,
    #[serde(default = "default_cache_hit_rate")]
    target_cache_hit_rate: f64,
}

fn default_bool_true() -> bool {
    true
}
fn default_metrics_interval() -> u64 {
    60
}
fn default_cache_hit_rate() -> f64 {
    0.85
}

impl Default for PerformanceSection {
    fn default() -> Self {
        Self {
            adaptive_cache: true,
            enable_metrics: true,
            metrics_interval: 60,
            target_cache_hit_rate: 0.85,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SecuritySection {
    #[serde(default = "default_audit_log_path")]
    audit_log_path: String,
    #[serde(default = "default_bool_true")]
    audit_logging: bool,
    #[serde(default = "default_ban_duration")]
    auth_ban_duration: u64,
    #[serde(default = "default_auth_failures")]
    max_auth_failures: u32,
}

fn default_audit_log_path() -> String {
    "./velocitydb_audit.log".to_string()
}
fn default_ban_duration() -> u64 {
    300
}
fn default_auth_failures() -> u32 {
    5
}

impl Default for SecuritySection {
    fn default() -> Self {
        Self {
            audit_log_path: default_audit_log_path(),
            audit_logging: true,
            auth_ban_duration: 300,
            max_auth_failures: 5,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DatabaseConfigSection {
    #[serde(default = "default_memtable")]
    max_memtable_size: usize,
    #[serde(default = "default_cache")]
    cache_size: usize,
    #[serde(default = "default_bloom")]
    bloom_false_positive_rate: f64,
    #[serde(default = "default_compaction")]
    compaction_threshold: usize,
    #[serde(default)]
    enable_compression: bool,
}

fn default_memtable() -> usize {
    10000
}
fn default_cache() -> usize {
    5000
}
fn default_bloom() -> f64 {
    0.001
}
fn default_compaction() -> usize {
    8
}

impl Default for DatabaseConfigSection {
    fn default() -> Self {
        Self {
            max_memtable_size: default_memtable(),
            cache_size: default_cache(),
            bloom_false_positive_rate: default_bloom(),
            compaction_threshold: default_compaction(),
            enable_compression: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AddonsSection {
    database: Option<DatabaseAddonConfig>,
    backup: Option<BackupAddonConfig>,
}

impl Default for ConfigFile {
    fn default() -> Self {
        let mut users = HashMap::new();
        users.insert(
            "admin".to_string(),
            "$argon2id$v=19$m=19456,t=2,p=1$GDWQpkPCnz9uM5W2SBpCmw$RNLHaiBA1s5wdbQSKJ28JzwD30wohA5KoB+W8MZOxic".to_string()
        );

        Self {
            server: ServerConfigSection::default(),
            logging: LoggingSection::default(),
            performance: PerformanceSection::default(),
            security: SecuritySection::default(),
            users,
            database: DatabaseConfigSection::default(),
            addons: AddonsSection::default(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { output } => {
            handle_init(&output).await?;
        }

        Commands::Server {
            config,
            data_dir,
            bind,
            verbose,
        } => {

            let file_config = if config.exists() {
                let content = std::fs::read_to_string(&config)?;
                toml::from_str::<ConfigFile>(&content)?
            } else {
                println!(
                    "{} Config file not found, creating default...",
                    "[WARN]".yellow()
                );
                let default_cfg = ConfigFile::default();
                let toml_string = toml::to_string_pretty(&default_cfg)?;
                std::fs::write(&config, toml_string)?;
                default_cfg
            };


            let log_level = if verbose {
                "debug"
            } else {
                &file_config.logging.level
            };

            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
                .init();

            println!(
                "{} Loading configuration from {:?}",
                "[CONFIG]".blue(),
                config
            );

            let mut server_config = ServerConfig::default();
            let bind_addr = bind.unwrap_or(file_config.server.bind_address.clone());
            server_config.bind_address = bind_addr.parse()?;
            server_config.max_connections = file_config.server.max_connections;
            server_config.connection_timeout =
                std::time::Duration::from_secs(file_config.server.connection_timeout_seconds);
            server_config.rate_limit_per_second = file_config.server.rate_limit_per_second;
            server_config.users = file_config.users;
            server_config.audit_log_path = file_config.security.audit_log_path;
            server_config.audit_logging = file_config.security.audit_logging;
            server_config.auth_ban_duration =
                std::time::Duration::from_secs(file_config.security.auth_ban_duration);
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
                metrics_interval: std::time::Duration::from_secs(
                    file_config.performance.metrics_interval,
                ),
                target_cache_hit_rate: file_config.performance.target_cache_hit_rate,
                wal_sync_mode: velocity::WalSyncMode::Batch,
            };

            println!(
                "{} Initializing storage at {:?}",
                "[STORAGE]".blue(),
                data_dir
            );
            let db = Velocity::open_with_config(&data_dir, db_config)?;


            let db_manager =
                std::sync::Arc::new(velocity::addon::DatabaseManager::new(db, config.clone()));


            let server = VelocityServer::new(db_manager.clone(), server_config)?;




            if let Some(backup_cfg) = &file_config.addons.backup {
                if backup_cfg.enabled {
                    let manager = db_manager.clone();
                    let interval = backup_cfg.interval_minutes;
                    println!(
                        "{} Automatic backups enabled ({} min interval)",
                        "[INFO]".green(),
                        interval
                    );

                    tokio::spawn(async move {
                        let mut interval_timer =
                            tokio::time::interval(std::time::Duration::from_secs(interval * 60));
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
            }


            let manager_for_watcher = db_manager.clone();
            let (tx, mut rx) = tokio::sync::mpsc::channel(1);

            let mut watcher =
                notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
                    Ok(event) => {
                        if event.kind.is_modify() {
                            let _ = tx.blocking_send(());
                        }
                    }
                    Err(e) => println!("{} Watcher error: {:?}", "[WARN]".yellow(), e),
                })?;

            use notify::Watcher;
            watcher.watch(&config, notify::RecursiveMode::NonRecursive)?;

            tokio::spawn(async move {
                while let Some(_) = rx.recv().await {
                    println!(
                        "{} Configuration change detected, reloading...",
                        "[CONFIG]".blue()
                    );
                    let _ = manager_for_watcher.reload_config();
                }
            });

            println!(
                "{} Velocity server starting on {}...",
                "[SERVER]".green(),
                bind_addr.bold()
            );
            server.start().await?;
        }

        Commands::CreateUser {
            username,
            password,
            config,
        } => {
            let user = if let Some(u) = username {
                u
            } else {
                Input::<String>::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter username")
                    .interact_text()?
            };

            let pass = if let Some(p) = password {
                p
            } else {
                Password::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter password")
                    .interact()?
            };

            let hash = hash_password(&pass)?;


            if config.exists() {
                let content = std::fs::read_to_string(&config)?;
                let mut toml_config: ConfigFile = toml::from_str(&content)?;

                toml_config.users.insert(user.clone(), hash.clone());

                let new_content = toml::to_string_pretty(&toml_config)?;
                std::fs::write(&config, new_content)?;

                println!(
                    "\n{} User {} created and saved to {:?}",
                    "[SUCCESS]".green(),
                    user.bold().cyan(),
                    config
                );
            } else {
                println!("\n{} Config file {:?} not found!", "[ERROR]".red(), config);
                println!(
                    "Run {} first or specify an existing config file.",
                    "velocity init".yellow()
                );
                println!("\nUser Hash (copy manually if needed):");
                println!("{} = \"{}\"", user, hash);
            }
        }

        Commands::Addon { subcommand } => match subcommand {
            AddonCommands::List { config } => {
                if !config.exists() {
                    return Err(format!("Config file {:?} not found!", config).into());
                }
                let content = std::fs::read_to_string(&config)?;
                let toml_config: ConfigFile = toml::from_str(&content)?;

                println!("{} Available Addons:", "[ADDON]".blue());
                let db_status = if toml_config
                    .addons
                    .database
                    .as_ref()
                    .map(|a| a.enabled)
                    .unwrap_or(false)
                {
                    "Enabled".green()
                } else {
                    "Disabled".red()
                };
                let backup_status = if toml_config
                    .addons
                    .backup
                    .as_ref()
                    .map(|a| a.enabled)
                    .unwrap_or(false)
                {
                    "Enabled".green()
                } else {
                    "Disabled".red()
                };

                println!("  - {}: {}", "database".bold(), db_status);
                println!("  - {}: {}", "backup".bold(), backup_status);
            }
            AddonCommands::Enable { name, config } => {
                if !config.exists() {
                    return Err(format!("Config file {:?} not found!", config).into());
                }
                let content = std::fs::read_to_string(&config)?;
                let mut toml_config: ConfigFile = toml::from_str(&content)?;

                match name.to_lowercase().as_str() {
                    "database" => {
                        let mut addon = toml_config.addons.database.unwrap_or_default();
                        addon.enabled = true;
                        toml_config.addons.database = Some(addon);
                    }
                    "backup" => {
                        let mut addon = toml_config.addons.backup.unwrap_or_default();
                        addon.enabled = true;
                        toml_config.addons.backup = Some(addon);
                    }
                    _ => return Err(format!("Unknown addon: {}", name).into()),
                }

                let new_content = toml::to_string_pretty(&toml_config)?;
                std::fs::write(&config, new_content)?;
                println!(
                    "{} Addon {} enabled.",
                    "[SUCCESS]".green(),
                    name.bold().cyan()
                );
            }
            AddonCommands::Disable { name, config } => {
                if !config.exists() {
                    return Err(format!("Config file {:?} not found!", config).into());
                }
                let content = std::fs::read_to_string(&config)?;
                let mut toml_config: ConfigFile = toml::from_str(&content)?;

                match name.to_lowercase().as_str() {
                    "database" => {
                        if let Some(mut addon) = toml_config.addons.database {
                            addon.enabled = false;
                            toml_config.addons.database = Some(addon);
                        }
                    }
                    "backup" => {
                        if let Some(mut addon) = toml_config.addons.backup {
                            addon.enabled = false;
                            toml_config.addons.backup = Some(addon);
                        }
                    }
                    _ => return Err(format!("Unknown addon: {}", name).into()),
                }

                let new_content = toml::to_string_pretty(&toml_config)?;
                std::fs::write(&config, new_content)?;
                println!(
                    "{} Addon {} disabled.",
                    "[SUCCESS]".green(),
                    name.bold().cyan()
                );
            }
        },

        Commands::Backup { config, data_dir } => {
            let db_config = VelocityConfig::default();
            let db = Velocity::open_with_config(&data_dir, db_config)?;
            let manager = velocity::addon::DatabaseManager::new(db, config);

            println!("{} Starting manual backup...", "[BACKUP]".blue());
            match manager.backup_all_databases() {
                Ok(succ) => {
                    println!(
                        "{} Backup completed successfully for {} databases.",
                        "[SUCCESS]".green(),
                        succ.len()
                    );
                    for name in succ {
                        println!("  - {}", name.cyan());
                    }
                }
                Err(e) => println!("{} Backup failed: {}", "[ERROR]".red(), e),
            }
        }

        Commands::Monitor { config, data_dir } => {
            if !config.exists() {
                return Err(format!("Config file {:?} not found!", config).into());
            }

            let content = std::fs::read_to_string(&config)?;
            let toml_config: ConfigFile = toml::from_str(&content)?;

            let velocity_config = VelocityConfig {
                max_memtable_size: toml_config.database.max_memtable_size,
                cache_size: toml_config.database.cache_size,
                bloom_false_positive_rate: toml_config.database.bloom_false_positive_rate,
                compaction_threshold: toml_config.database.compaction_threshold,
                enable_compression: toml_config.database.enable_compression,
                memory_only_mode: false,
                batch_wal_writes: true,
                adaptive_cache: toml_config.performance.adaptive_cache,
                enable_metrics: toml_config.performance.enable_metrics,
                metrics_interval: std::time::Duration::from_secs(
                    toml_config.performance.metrics_interval,
                ),
                target_cache_hit_rate: toml_config.performance.target_cache_hit_rate,
                wal_sync_mode: velocity::WalSyncMode::Batch,
            };

            let db = Velocity::open_with_config(&data_dir, velocity_config)?;
            let manager =
                std::sync::Arc::new(velocity::addon::DatabaseManager::new(db, config.clone()));

            let stats = manager.stats();
            let wal_report = manager
                .get_database("default")
                .ok_or_else(|| VeloError::InvalidOperation("Default database missing".to_string()))?
                .wal_integrity_report()?;

            let backup_cfg = toml_config.addons.backup.unwrap_or_default();
            let multi_tenant = toml_config
                .addons
                .database
                .as_ref()
                .map(|a| a.enabled)
                .unwrap_or(false);
            let addon_status = manager
                .list_addons()
                .into_iter()
                .map(|(name, enabled)| format!("{}={}", name, if enabled { "on" } else { "off" }))
                .collect::<Vec<_>>()
                .join(", ");

            println!("\n{} Monitoring snapshot", "[MONITOR]".cyan().bold());
            println!("Config file: {}", config.display());
            println!(
                "Studio console: `velocity studio --port 3000 --config {} --data-dir {}`",
                config.display(),
                data_dir.display()
            );
            println!(
                "Multi-tenant addon: {}",
                if multi_tenant { "enabled" } else { "disabled" }
            );
            println!("Addon states: {}", addon_status);

            println!("\n{} Metrics", "[METRICS]".cyan().bold());
            println!(
                "Collector: {}",
                if toml_config.performance.enable_metrics {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!(
                "Interval: {} seconds",
                toml_config.performance.metrics_interval
            );
            println!(
                "Target cache hit rate: {:.2}",
                toml_config.performance.target_cache_hit_rate
            );
            println!("Aggregate stats:");
            println!("  - memtable entries: {}", stats.memtable_entries);
            println!("  - sstable count: {}", stats.sstable_count);
            println!("  - cache entries: {}", stats.cache_entries);
            println!("  - total SSTable size: {} bytes", stats.total_sstable_size);

            println!("\n{} Backup strategy", "[BACKUP]".cyan().bold());
            println!("Enabled: {}", backup_cfg.enabled);
            println!("Path: {:?}", backup_cfg.backup_path);
            println!("Interval: {} minutes", backup_cfg.interval_minutes);
            println!(
                "Scope: {}",
                if backup_cfg.backup_all {
                    "all managed databases"
                } else {
                    "targeted subset"
                }
            );
            if !backup_cfg.target_databases.is_empty() {
                println!("Targets: {}", backup_cfg.target_databases.join(", "));
            }

            println!("\n{} Upgrade story", "[UPGRADE]".cyan().bold());
            println!("  1. Stop the running binary.");
            println!("  2. Pull latest code (`git pull`).");
            println!("  3. Rebuild via `make release` or `cargo install --path .`.");
            println!("  4. Restart against the same data directory to let WAL replay finish.");
            println!("  - Docker users: `docker-compose down && docker-compose up -d --build`.");

            println!("\n{} Corruption detection", "[INTEGRITY]".cyan().bold());
            println!("WAL file: {}/velocity.wal", data_dir.display());
            println!("Total WAL entries: {}", wal_report.total_records);
            println!("Checksum mismatches: {}", wal_report.corrupted_records);
            println!(
                "Incomplete/truncated WAL records: {}",
                wal_report.truncated_records
            );
            if !wal_report.corrupted_keys.is_empty() {
                println!(
                    "Sample inconsistent keys: {}",
                    wal_report.corrupted_keys.join(", ")
                );
            }
        }

        Commands::Benchmark {
            data_dir,
            operations,
            mode,
            cache_size,
        } => {
            run_benchmark(&data_dir, operations, mode, cache_size).await?;
        }

        Commands::Studio {
            port,
            config,
            data_dir,
        } => {
            let db_config = VelocityConfig::default();
            let db = Velocity::open_with_config(&data_dir, db_config)?;
            let manager =
                std::sync::Arc::new(velocity::addon::DatabaseManager::new(db, config.clone()));

            let addr = format!("127.0.0.1:{}", port).parse()?;
            velocity::studio::start_studio(addr, manager, config).await?;
        }
    }

    Ok(())
}

async fn handle_init(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{} {} VelocityDB Interactive Setup\n",
        "[INIT]".blue(),
        "Initialization:".bold()
    );

    if path.exists() {
        let overwrite = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("File {:?} already exists. Overwrite?", path))
            .default(false)
            .interact()?;

        if !overwrite {
            println!("Aborted.");
            return Ok(());
        }
    }

    let bind_address: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Bind address (e.g. 127.0.0.1:2005)")
        .default("127.0.0.1:2005".into())
        .interact_text()?;

    let mode = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Operation Mode")
        .items(&["Single Database", "Multi-Tenant (Addon Enabled)"])
        .default(0)
        .interact()?;

    let mut config = ConfigFile::default();
    config.server.bind_address = bind_address;

    if mode == 1 {
        config.addons.database = Some(DatabaseAddonConfig::default());
        println!("{} Multi-tenant mode enabled.", "[INFO]".green());
    }


    let enable_backup = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Enable Automatic Backup System?")
        .default(true)
        .interact()?;

    if enable_backup {
        let mut backup_cfg = BackupAddonConfig::default();
        backup_cfg.enabled = true;

        let backup_mode = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Backup Scope")
            .items(&["Backup All Databases", "Select Specific Databases"])
            .default(0)
            .interact()?;

        if backup_mode == 1 {
            backup_cfg.backup_all = false;
            let db_names_str: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Target Databases (comma separated)")
                .interact_text()?;

            backup_cfg.target_databases = db_names_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }

        let interval: u64 = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Backup Interval (minutes)")
            .default(60)
            .interact_text()?;

        backup_cfg.interval_minutes = interval;
        config.addons.database = Some(config.addons.database.unwrap_or_default());

        config.addons.backup = Some(backup_cfg);
        println!("{} Backup system configured.", "[INFO]".green());
    }

    let admin_pass = Password::with_theme(&ColorfulTheme::default())
        .with_prompt("Set admin password")
        .interact()?;

    config
        .users
        .insert("admin".to_string(), hash_password(&admin_pass)?);

    let toml_string = toml::to_string_pretty(&config)?;
    std::fs::write(path, toml_string)?;

    println!(
        "\n{} Configuration saved to {:?}",
        "[SUCCESS]".green(),
        path
    );
    println!(
        "You can now start the server with: {} server",
        "velocity".yellow()
    );

    Ok(())
}

async fn run_benchmark(
    data_dir: &PathBuf,
    operations: usize,
    mode: String,
    cache_size: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{} {}",
        "[BENCH]".yellow(),
        "VelocityDB Performance Benchmark".bold()
    );
    println!("{}", "=================================".dimmed());
    println!("{} Mode: {}", "[INFO]".blue(), mode.to_uppercase().green());

    let has_cache_limit = cache_size.is_some();
    let cache_limit = cache_size.unwrap_or(100_000);

    let mut config = VelocityConfig {
        max_memtable_size: 200_000,
        cache_size: cache_limit,
        bloom_false_positive_rate: 0.001,
        compaction_threshold: 4,
        enable_compression: false,
        memory_only_mode: false,
        batch_wal_writes: true,
        adaptive_cache: false,
        enable_metrics: false,
        metrics_interval: Default::default(),
        target_cache_hit_rate: 0.0,
        wal_sync_mode: velocity::WalSyncMode::Batch,
    };

    println!(
        "{} Configuration: Cache={}, Sync=Batch",
        "[INFO]".blue(),
        if has_cache_limit {
            format!("{} (Constrained)", cache_limit).yellow()
        } else {
            "Unlimited".into()
        }
    );


    if data_dir.exists() {
        std::fs::remove_dir_all(data_dir)?;
    }
    std::fs::create_dir_all(data_dir)?;

    if mode == "mixed" {
        let db = Velocity::open_with_config(data_dir, config)?;
        println!(
            "{} Running mixed R/W benchmark (70% Read / 30% Write)...",
            "[MIX]".blue()
        );

        let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(42);
        use rand::Rng;

        let start = std::time::Instant::now();
        let mut read_latencies = Vec::with_capacity(operations);
        let mut write_latencies = Vec::with_capacity(operations);

        for i in 0..operations {
            let key = format!("key_{:06}", i % 10_000);
            let is_write = rng.gen_bool(0.3);

            let op_start = std::time::Instant::now();
            if is_write {
                let value = format!("val_{}", i).into_bytes();
                db.put(key, value)?;
                write_latencies.push(op_start.elapsed());
            } else {
                let _ = db.get(&key)?;
                read_latencies.push(op_start.elapsed());
            }
        }

        let duration = start.elapsed();
        let ops_sec = operations as f64 / duration.as_secs_f64();


        read_latencies.sort();
        write_latencies.sort();

        let p50_r = read_latencies
            .get(read_latencies.len() / 2)
            .unwrap_or(&std::time::Duration::ZERO);
        let p99_r = read_latencies
            .get(read_latencies.len() * 99 / 100)
            .unwrap_or(&std::time::Duration::ZERO);
        let p50_w = write_latencies
            .get(write_latencies.len() / 2)
            .unwrap_or(&std::time::Duration::ZERO);
        let p99_w = write_latencies
            .get(write_latencies.len() * 99 / 100)
            .unwrap_or(&std::time::Duration::ZERO);

        println!("\n{}", "Mixed Workload Results:".bold().green());
        println!(
            "  Throughput: {} ops/sec",
            format!("{:.0}", ops_sec).bold().yellow()
        );
        println!("  Read Latency (p50): {:?}", p50_r);
        println!("  Read Latency (p99): {:?}", p99_r);
        println!("  Write Latency (p50): {:?}", p50_w);
        println!("  Write Latency (p99): {:?}", p99_w);

        db.close()?;
    } else if mode == "persistent" {
        let db = Velocity::open_with_config(data_dir, config.clone())?;

        println!("{} Writing {} records...", "[WRITE]".blue(), operations);
        for i in 0..operations {
            let key = format!("key_{:06}", i);
            let value = vec![0u8; 128];
            db.put(key, value)?;
        }

        println!("{} Flushing to SSTables (Disk)...", "[FLUSH]".yellow());
        let flush_start = std::time::Instant::now();
        db.flush()?;
        println!("  Flush took: {:?}", flush_start.elapsed());


        db.close()?;
        drop(db);
        println!(
            "{} Re-opening database (Cold Start)...",
            "[RELOAD]".yellow()
        );

        let db = Velocity::open_with_config(data_dir, config)?;

        println!(
            "{} Reading {} records from SSTables...",
            "[READ]".blue(),
            operations
        );
        let start = std::time::Instant::now();
        for i in 0..operations {
            let key = format!("key_{:06}", i);
            db.get(&key)?;
        }
        let duration = start.elapsed();

        println!("\n{}", "Cold Read Results (SSTable):".bold().green());
        println!("  Duration:   {:?}", duration);
        println!(
            "  Throughput: {} ops/sec",
            format!("{:.0}", operations as f64 / duration.as_secs_f64())
                .bold()
                .yellow()
        );
        println!(
            "  Latency:    {:.2} μs/op",
            duration.as_micros() as f64 / operations as f64
        );


        let stats = db.stats();
        println!("\n{}", "Storage Stats:".bold().cyan());
        println!("  SSTables: {} files", stats.sstable_count);
        println!(
            "  Total Size: {:.2} MB",
            stats.total_sstable_size as f64 / 1024.0 / 1024.0
        );
    } else {

        let db = Velocity::open_with_config(data_dir, config)?;

        println!("{} Running write benchmark...", "[WRITE]".blue());
        let start = std::time::Instant::now();
        for i in 0..operations {
            db.put(format!("k_{}", i), vec![0u8; 64])?;
        }
        let dur = start.elapsed();
        println!(
            "  Throughput: {:.0} ops/sec",
            operations as f64 / dur.as_secs_f64()
        );

        println!("{} Waiting for WAL sync...", "[WAIT]".yellow());
        std::thread::sleep(std::time::Duration::from_millis(500));

        println!("{} Running read benchmark...", "[READ]".blue());
        let start = std::time::Instant::now();
        for i in 0..operations {
            db.get(&format!("k_{}", i))?;
        }
        let dur = start.elapsed();
        println!(
            "  Throughput: {:.0} ops/sec",
            operations as f64 / dur.as_secs_f64()
        );
        println!(
            "  Latency:    {:.2} μs/op",
            dur.as_micros() as f64 / operations as f64
        );
        db.close()?;
    }

    Ok(())
}
