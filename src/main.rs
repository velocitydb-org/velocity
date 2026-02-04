use std::path::PathBuf;
use clap::{Parser, Subcommand};
use tokio;
use env_logger;

use velocity::{Velocity, VelocityConfig};
use velocity::server::{VelocityServer, ServerConfig, hash_password};

#[derive(Parser)]
#[command(name = "velocitydb")]
#[command(about = "VelocityDB - High-performance database server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the database server
    Server {
        /// Configuration file path
        #[arg(short, long, default_value = "velocity.toml")]
        config: PathBuf,
        
        /// Database directory
        #[arg(short, long, default_value = "./velocitydb")]
        data_dir: PathBuf,
        
        /// Bind address
        #[arg(short, long, default_value = "127.0.0.1:5432")]
        bind: String,
        
        /// Enable verbose logging
        #[arg(short, long)]
        verbose: bool,
    },
    
    /// Create a new user
    CreateUser {
        /// Username
        #[arg(short, long)]
        username: String,
        
        /// Password
        #[arg(short, long)]
        password: String,
    },
    
    /// Run performance benchmarks
    Benchmark {
        /// Database directory
        #[arg(short, long, default_value = "./benchmark_db")]
        data_dir: PathBuf,
        
        /// Number of operations
        #[arg(short, long, default_value = "10000")]
        operations: usize,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Server { config, data_dir, bind, verbose } => {
            // Initialize logging
            if verbose {
                env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
            } else {
                env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
            }

            // Load configuration
            let server_config = load_server_config(&config, &bind)?;
            
            // Initialize database
            let db_config = VelocityConfig {
                max_memtable_size: 10_000,
                cache_size: 5_000,
                bloom_false_positive_rate: 0.001,
                compaction_threshold: 8,
                enable_compression: false,
                memory_only_mode: false,
                batch_wal_writes: true,
            };
            
            let db = Velocity::open_with_config(&data_dir, db_config)?;
            log::info!("Database initialized at {:?}", data_dir);
            
            // Start server
            let server = VelocityServer::new(db, server_config)?;
            log::info!("Starting VelocityDB server...");
            
            server.start().await?;
        }
        
        Commands::CreateUser { username, password } => {
            let hash = hash_password(&password)?;
            println!("User: {}", username);
            println!("Password Hash: {}", hash);
            println!("\nAdd this to your velocity.toml config:");
            println!("[users]");
            println!("{} = \"{}\"", username, hash);
        }
        
        Commands::Benchmark { data_dir, operations } => {
            run_benchmark(&data_dir, operations).await?;
        }
    }

    Ok(())
}

fn load_server_config(config_path: &PathBuf, bind_address: &str) -> Result<ServerConfig, Box<dyn std::error::Error>> {
    let mut config = ServerConfig::default();
    config.bind_address = bind_address.parse()?;
    
    // Try to load from config file
    if config_path.exists() {
        let config_content = std::fs::read_to_string(config_path)?;
        let toml_config: toml::Value = toml::from_str(&config_content)?;
        
        // Parse configuration
        if let Some(server) = toml_config.get("server") {
            if let Some(max_conn) = server.get("max_connections") {
                config.max_connections = max_conn.as_integer().unwrap_or(1000) as usize;
            }
            if let Some(rate_limit) = server.get("rate_limit_per_second") {
                config.rate_limit_per_second = rate_limit.as_integer().unwrap_or(1000) as u32;
            }
        }
        
        if let Some(users) = toml_config.get("users") {
            if let Some(users_table) = users.as_table() {
                config.users.clear();
                for (username, password_hash) in users_table {
                    if let Some(hash_str) = password_hash.as_str() {
                        config.users.insert(username.clone(), hash_str.to_string());
                    }
                }
            }
        }
    } else {
        // Create default config file
        let default_config = r#"[server]
max_connections = 1000
rate_limit_per_second = 1000
connection_timeout_seconds = 300

[users]
admin = "$argon2id$v=19$m=65536,t=3,p=4$salt$hash"

[database]
max_memtable_size = 10000
cache_size = 5000
bloom_false_positive_rate = 0.001
compaction_threshold = 8
"#;
        std::fs::write(config_path, default_config)?;
        log::info!("Created default configuration at {:?}", config_path);
    }
    
    Ok(config)
}

async fn run_benchmark(data_dir: &PathBuf, operations: usize) -> Result<(), Box<dyn std::error::Error>> {
    println!("VelocityDB Performance Benchmark");
    println!("=================================");
    println!("Mode: PRODUCTION (Adaptive Batch Flushing + WAL)");
    println!("Strategy: 2,4,8,16,32,64,128 packet adaptive flushing");
    println!();
    
    let config = VelocityConfig {
        max_memtable_size: 200_000,  // Large memtable
        cache_size: 100_000,         // Large cache
        bloom_false_positive_rate: 0.001,
        compaction_threshold: 128,
        enable_compression: false,
        memory_only_mode: false,     // WAL ENABLED for durability!
        batch_wal_writes: true,      // Adaptive batching
    };
    
    let db = Velocity::open_with_config(data_dir, config)?;
    
    // Write benchmark - simple individual puts for maximum speed
    println!("\nRunning write benchmark ({} operations)...", operations);
    let start = std::time::Instant::now();
    
    for i in 0..operations {
        let key = format!("bench_key_{:06}", i);
        let value = format!("benchmark_value_{}", i).into_bytes();
        db.put(key, value)?;
    }
    
    // CRITICAL: Wait for async writes to actually complete!
    println!("Waiting for async writes to complete...");
    std::thread::sleep(std::time::Duration::from_secs(5));
    
    let write_duration = start.elapsed();
    let write_ops_per_sec = operations as f64 / write_duration.as_secs_f64();
    
    println!("Write Results:");
    println!("  Duration: {:?}", write_duration);
    println!("  Ops/sec: {:.0}", write_ops_per_sec);
    println!("  Avg latency: {:.2} μs", write_duration.as_micros() as f64 / operations as f64);
    
    // Verify data is actually written
    let stats_after_write = db.stats();
    println!("  Verified memtable entries: {}", stats_after_write.memtable_entries);
    
    if stats_after_write.memtable_entries != operations {
        println!("  ⚠️  WARNING: Not all writes completed! Expected {}, got {}", 
                 operations, stats_after_write.memtable_entries);
    }
    
    println!("Write Results:");
    println!("  Duration: {:?}", write_duration);
    println!("  Ops/sec: {:.0}", write_ops_per_sec);
    println!("  Avg latency: {:.2} μs", write_duration.as_micros() as f64 / operations as f64);
    
    // Read benchmark
    println!("\nRunning read benchmark ({} operations)...", operations);
    let start = std::time::Instant::now();
    
    for i in 0..operations {
        let key = format!("bench_key_{:06}", i);
        let _ = db.get(&key)?;
    }
    
    let read_duration = start.elapsed();
    let read_ops_per_sec = operations as f64 / read_duration.as_secs_f64();
    
    println!("Read Results:");
    println!("  Duration: {:?}", read_duration);
    println!("  Ops/sec: {:.0}", read_ops_per_sec);
    println!("  Avg latency: {:.2} μs", read_duration.as_micros() as f64 / operations as f64);
    
    // Database stats
    let stats = db.stats();
    println!("\nDatabase Statistics:");
    println!("  Memtable entries: {}", stats.memtable_entries);
    println!("  SSTable count: {}", stats.sstable_count);
    println!("  Cache entries: {}", stats.cache_entries);
    println!("  Total SSTable size: {} bytes", stats.total_sstable_size);
    
    db.close()?;
    
    println!("\nBenchmark completed!");
    Ok(())
}