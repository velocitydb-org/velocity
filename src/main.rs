use velocity::VeloError;
use clap::{Parser, Subcommand};
use colored::*;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Password, Select};
use env_logger;
use std::path::{Path, PathBuf};
use tokio;
use velocity::addon::BackupAddonConfig;
use velocity::addon::DatabaseAddonConfig;
use velocity::server::{hash_password, ServerConfig, VelocityServer};
use velocity::{Velocity, VelocityConfig};

mod config;
mod service_runner;
mod setup;
use crate::config::ConfigFile;
use crate::service_runner::{run_velocity_service, ServiceSpec};
use crate::setup::{print_default_paths, run_setup_install, SetupInstallSpec};

#[derive(Parser)]
#[command(name = "velocity")]
#[command(version, about = "VelocityDB - Modern, High-performance Database")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Database lifecycle commands")]
    Db {
        #[command(subcommand)]
        subcommand: DbCommands,
    },

    #[command(about = "Administration commands")]
    Admin {
        #[command(subcommand)]
        subcommand: AdminCommands,
    },

    #[command(about = "Operations and service commands")]
    Ops {
        #[command(subcommand)]
        subcommand: OpsCommands,
    },

    #[command(about = "Install and system setup")]
    Setup {
        #[command(subcommand)]
        subcommand: SetupCommands,
    },

    #[command(hide = true)]
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

    #[command(hide = true)]
    CreateUser {
        #[arg(short, long)]
        username: Option<String>,
        #[arg(short, long)]
        password: Option<String>,
        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
    },

    #[command(hide = true)]
    Init {
        #[arg(short, long, default_value = "velocity.toml")]
        output: PathBuf,
    },

    #[command(hide = true)]
    Addon {
        #[command(subcommand)]
        subcommand: AddonCommands,
    },

    #[command(hide = true)]
    Backup {
        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
    },

    #[command(hide = true)]
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

    #[command(hide = true)]
    Studio {
        #[arg(short, long, default_value = "3000")]
        port: u16,
        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
    },

    #[command(hide = true)]
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

#[derive(Subcommand)]
enum DbCommands {
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
    Init {
        #[arg(short, long, default_value = "velocity.toml")]
        output: PathBuf,
    },
    Studio {
        #[arg(short, long, default_value = "3000")]
        port: u16,
        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum AdminCommands {
    CreateUser {
        #[arg(short, long)]
        username: Option<String>,
        #[arg(short, long)]
        password: Option<String>,
        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
    },
    Addon {
        #[command(subcommand)]
        subcommand: AddonCommands,
    },
}

#[derive(Subcommand)]
enum OpsCommands {
    Backup {
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
    Service {
        #[command(subcommand)]
        subcommand: ServiceCommands,
    },
}

#[derive(Subcommand)]
enum ServiceCommands {
    Run {
        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
        #[arg(short, long)]
        bind: Option<String>,
        #[arg(short, long)]
        verbose: bool,
        #[arg(long)]
        pid_file: Option<PathBuf>,
        #[arg(long, default_value_t = true)]
        watch_config: bool,
    },
    Install {
        #[arg(short, long, default_value = "./service_templates")]
        template_dir: PathBuf,
        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
        #[arg(short, long)]
        bind: Option<String>,
    },
    Uninstall {
        #[arg(short, long, default_value = "./service_templates")]
        template_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum SetupCommands {
    Install {
        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
        #[arg(short, long)]
        bind: Option<String>,
        #[arg(long)]
        bin_dir: Option<PathBuf>,
        #[arg(long)]
        service_file: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        no_service: bool,
    },
    Paths,
}

enum ResolvedCommand {
    Server {
        config: PathBuf,
        data_dir: PathBuf,
        bind: Option<String>,
        verbose: bool,
    },
    CreateUser {
        username: Option<String>,
        password: Option<String>,
        config: PathBuf,
    },
    Init {
        output: PathBuf,
    },
    Addon {
        subcommand: AddonCommands,
    },
    Backup {
        config: PathBuf,
        data_dir: PathBuf,
    },
    Benchmark {
        data_dir: PathBuf,
        operations: usize,
        mode: String,
        cache_size: Option<usize>,
    },
    Studio {
        port: u16,
        config: PathBuf,
        data_dir: PathBuf,
    },
    Monitor {
        config: PathBuf,
        data_dir: PathBuf,
    },
    ServiceRun {
        config: PathBuf,
        data_dir: PathBuf,
        bind: Option<String>,
        verbose: bool,
        pid_file: Option<PathBuf>,
        watch_config: bool,
    },
    ServiceInstall {
        template_dir: PathBuf,
        config: PathBuf,
        data_dir: PathBuf,
        bind: Option<String>,
    },
    ServiceUninstall {
        template_dir: PathBuf,
    },
    SetupInstall {
        config: PathBuf,
        data_dir: PathBuf,
        bind: Option<String>,
        bin_dir: Option<PathBuf>,
        service_file: Option<PathBuf>,
        no_service: bool,
    },
    SetupPaths,
}

fn resolve_command(command: Commands) -> ResolvedCommand {
    match command {
        Commands::Db { subcommand } => match subcommand {
            DbCommands::Server {
                config,
                data_dir,
                bind,
                verbose,
            } => ResolvedCommand::Server {
                config,
                data_dir,
                bind,
                verbose,
            },
            DbCommands::Init { output } => ResolvedCommand::Init { output },
            DbCommands::Studio {
                port,
                config,
                data_dir,
            } => ResolvedCommand::Studio {
                port,
                config,
                data_dir,
            },
        },
        Commands::Admin { subcommand } => match subcommand {
            AdminCommands::CreateUser {
                username,
                password,
                config,
            } => ResolvedCommand::CreateUser {
                username,
                password,
                config,
            },
            AdminCommands::Addon { subcommand } => ResolvedCommand::Addon { subcommand },
        },
        Commands::Ops { subcommand } => match subcommand {
            OpsCommands::Backup { config, data_dir } => ResolvedCommand::Backup { config, data_dir },
            OpsCommands::Monitor { config, data_dir } => {
                ResolvedCommand::Monitor { config, data_dir }
            }
            OpsCommands::Benchmark {
                data_dir,
                operations,
                mode,
                cache_size,
            } => ResolvedCommand::Benchmark {
                data_dir,
                operations,
                mode,
                cache_size,
            },
            OpsCommands::Service { subcommand } => match subcommand {
                ServiceCommands::Run {
                    config,
                    data_dir,
                    bind,
                    verbose,
                    pid_file,
                    watch_config,
                } => ResolvedCommand::ServiceRun {
                    config,
                    data_dir,
                    bind,
                    verbose,
                    pid_file,
                    watch_config,
                },
                ServiceCommands::Install {
                    template_dir,
                    config,
                    data_dir,
                    bind,
                } => ResolvedCommand::ServiceInstall {
                    template_dir,
                    config,
                    data_dir,
                    bind,
                },
                ServiceCommands::Uninstall { template_dir } => {
                    ResolvedCommand::ServiceUninstall { template_dir }
                }
            },
        },
        Commands::Setup { subcommand } => match subcommand {
            SetupCommands::Install {
                config,
                data_dir,
                bind,
                bin_dir,
                service_file,
                no_service,
            } => ResolvedCommand::SetupInstall {
                config,
                data_dir,
                bind,
                bin_dir,
                service_file,
                no_service,
            },
            SetupCommands::Paths => ResolvedCommand::SetupPaths,
        },
        Commands::Server {
            config,
            data_dir,
            bind,
            verbose,
        } => ResolvedCommand::Server {
            config,
            data_dir,
            bind,
            verbose,
        },
        Commands::CreateUser {
            username,
            password,
            config,
        } => ResolvedCommand::CreateUser {
            username,
            password,
            config,
        },
        Commands::Init { output } => ResolvedCommand::Init { output },
        Commands::Addon { subcommand } => ResolvedCommand::Addon { subcommand },
        Commands::Backup { config, data_dir } => ResolvedCommand::Backup { config, data_dir },
        Commands::Benchmark {
            data_dir,
            operations,
            mode,
            cache_size,
        } => ResolvedCommand::Benchmark {
            data_dir,
            operations,
            mode,
            cache_size,
        },
        Commands::Studio {
            port,
            config,
            data_dir,
        } => ResolvedCommand::Studio {
            port,
            config,
            data_dir,
        },
        Commands::Monitor { config, data_dir } => ResolvedCommand::Monitor { config, data_dir },
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let command = resolve_command(cli.command);

    match command {
        ResolvedCommand::Init { output } => {
            handle_init(&output).await?;
        }

        ResolvedCommand::Server {
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

            let background_service_cfg = file_config
                .addons
                .background_service
                .clone()
                .unwrap_or_default();
            if background_service_cfg.enabled {
                println!(
                    "{} background-service addon enabled; launching service mode.",
                    "[SERVICE]".green()
                );
                run_velocity_service(ServiceSpec {
                    config_path: config,
                    data_dir,
                    bind,
                    verbose,
                    pid_file: Some(background_service_cfg.pid_file),
                    watch_config: background_service_cfg.watch_config,
                })
                .await?;
                return Ok(());
            }


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

        ResolvedCommand::CreateUser {
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

        ResolvedCommand::Addon { subcommand } => match subcommand {
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
                let background_service_status = if toml_config
                    .addons
                    .background_service
                    .as_ref()
                    .map(|a| a.enabled)
                    .unwrap_or(true)
                {
                    "Enabled".green()
                } else {
                    "Disabled".red()
                };

                println!("  - {}: {}", "database".bold(), db_status);
                println!("  - {}: {}", "backup".bold(), backup_status);
                println!(
                    "  - {}: {}",
                    "background-service".bold(),
                    background_service_status
                );
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
                    "background-service" | "background_service" => {
                        let mut addon = toml_config.addons.background_service.unwrap_or_default();
                        addon.enabled = true;
                        toml_config.addons.background_service = Some(addon);
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
                    "background-service" | "background_service" => {
                        let mut addon = toml_config.addons.background_service.unwrap_or_default();
                        addon.enabled = false;
                        toml_config.addons.background_service = Some(addon);
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

        ResolvedCommand::Backup { config, data_dir } => {
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

        ResolvedCommand::Monitor { config, data_dir } => {
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

        ResolvedCommand::Benchmark {
            data_dir,
            operations,
            mode,
            cache_size,
        } => {
            run_benchmark(&data_dir, operations, mode, cache_size).await?;
        }

        ResolvedCommand::Studio {
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
        ResolvedCommand::ServiceRun {
            config,
            data_dir,
            bind,
            verbose,
            pid_file,
            watch_config,
        } => {
            run_velocity_service(ServiceSpec {
                config_path: config,
                data_dir,
                bind,
                verbose,
                pid_file,
                watch_config,
            })
            .await?;
        }
        ResolvedCommand::ServiceInstall {
            template_dir,
            config,
            data_dir,
            bind,
        } => {
            install_service_templates(&template_dir, &config, &data_dir, bind.as_ref())?;
        }
        ResolvedCommand::ServiceUninstall { template_dir } => {
            uninstall_service_templates(&template_dir)?;
        }
        ResolvedCommand::SetupInstall {
            config,
            data_dir,
            bind,
            bin_dir,
            service_file,
            no_service,
        } => {
            run_setup_install(SetupInstallSpec {
                config,
                data_dir,
                bind,
                bin_dir,
                service_file,
                with_service: !no_service,
            })?;
        }
        ResolvedCommand::SetupPaths => {
            print_default_paths();
        }
    }

    Ok(())
}

fn install_service_templates(
    dir: &Path,
    config: &Path,
    data_dir: &Path,
    bind: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dir)?;
    let exe = std::env::current_exe()?;
    let exe_str = exe.display().to_string();
    let unit = generate_systemd_unit(&exe_str, config, data_dir, bind);
    let script = generate_windows_script(&exe_str, config, data_dir, bind);
    std::fs::write(dir.join("velocity.service"), unit)?;
    std::fs::write(dir.join("install-velocity.ps1"), script)?;
    println!(
        "{} Service templates written to {:?}",
        "[SUCCESS]".green(),
        dir
    );
    Ok(())
}

fn uninstall_service_templates(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
        println!("{} Removed templates under {:?}", "[WARN]".yellow(), dir);
    } else {
        println!("{} No templates found at {:?}", "[WARN]".yellow(), dir);
    }
    Ok(())
}

fn generate_systemd_unit(
    exe_path: &str,
    config: &Path,
    data_dir: &Path,
    bind: Option<&String>,
) -> String {
    let bind_arg = bind
        .map(|b| format!(" --bind {}", b))
        .unwrap_or_default();

    format!(
        "[Unit]\n\
Description=VelocityDB service\n\
After=network.target\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart={} ops service run --config {} --data-dir {}{} --verbose\n\
Restart=on-failure\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        exe_path,
        config.display(),
        data_dir.display(),
        bind_arg
    )
}

fn generate_windows_script(
    exe_path: &str,
    config: &Path,
    data_dir: &Path,
    bind: Option<&String>,
) -> String {
    let bind_arg = bind
        .map(|b| format!(" --bind {}", b))
        .unwrap_or_default();

    format!(
        "param(\n    [string]$ServiceName = \"VelocityDB\",\n    [string]$DisplayName = \"VelocityDB Service\"\n)\n\n$binPath = \"{} ops service run --config {} --data-dir {}{} --verbose\"\n\nsc.exe create $ServiceName binPath= \"$binPath\" DisplayName= \"$DisplayName\" start= auto\nsc.exe description $ServiceName \"VelocityDB background service\"\n",
        exe_path,
        config.display(),
        data_dir.display(),
        bind_arg
    )
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

