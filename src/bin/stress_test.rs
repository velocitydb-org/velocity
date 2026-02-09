use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "velocitydb")]
#[command(version, about = "VelocityDB - Comprehensive Stress Test Benchmark")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /
    Stress {
        /
        #[arg(short, long, default_value = "./stress_test_db")]
        data_dir: PathBuf,

        /
        #[arg(long, default_value = "30")]
        sstable_count: usize,

        /
        #[arg(short, long, default_value = "50000")]
        operations: usize,

        /
        #[arg(long)]
        no_bloom: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Stress {
            data_dir,
            sstable_count,
            operations,
            no_bloom,
        } => {
            run_stress_test(&data_dir, sstable_count, operations, no_bloom).await?;
        }
    }

    Ok(())
}

async fn run_stress_test(
    data_dir: &PathBuf,
    target_sstables: usize,
    ops_per_phase: usize,
    disable_bloom: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Instant;
    use velocity::{Velocity, VelocityConfig};

    println!("{}", "=".repeat(70).bold());
    println!("{}", "  VelocityDB COMPREHENSIVE STRESS TEST".bold().cyan());
    println!("{}", "=".repeat(70).bold());
    println!();


    if data_dir.exists() {
        std::fs::remove_dir_all(data_dir)?;
    }
    std::fs::create_dir_all(data_dir)?;

    let bloom_rate = if disable_bloom { 1.0 } else { 0.001 };

    let config = VelocityConfig {
        max_memtable_size: 5000,
        cache_size: 10000,
        bloom_false_positive_rate: bloom_rate,
        compaction_threshold: 100,
        enable_compression: false,
        memory_only_mode: false,
        batch_wal_writes: true,
        adaptive_cache: false,
        enable_metrics: false,
        metrics_interval: std::time::Duration::from_secs(60),
        target_cache_hit_rate: 0.85,
        wal_sync_mode: velocity::WalSyncMode::Batch,
    };

    println!("{} Test Configuration:", "[CONFIG]".blue());
    println!(
        "  Target SSTables: {}",
        target_sstables.to_string().yellow()
    );
    println!("  Operations/Phase: {}", ops_per_phase.to_string().yellow());
    println!(
        "  Bloom Filter: {}",
        if disable_bloom {
            "DISABLED".red()
        } else {
            "ENABLED".green()
        }
    );
    println!(
        "  Memtable Size: {}",
        "5000 (small for many SSTables)".yellow()
    );
    println!();


    println!(
        "{} Building {} SSTables...",
        "[PHASE 1]".green().bold(),
        target_sstables
    );
    let db = Velocity::open_with_config(data_dir, config.clone())?;

    let records_per_sstable = 5000;
    let total_records = target_sstables * records_per_sstable;

    let build_start = Instant::now();
    for i in 0..total_records {
        let key = format!("key_{:08}", i);
        let value = format!("value_data_{:08}_padding_to_make_realistic_size", i).into_bytes();
        db.put(key, value)?;


        if (i + 1) % records_per_sstable == 0 {
            db.flush()?;
            let stats = db.stats();
            print!(
                "\r  Progress: {}/{} SSTables created",
                stats.sstable_count, target_sstables
            );
            use std::io::Write;
            std::io::stdout().flush()?;
        }
    }
    println!();

    let build_duration = build_start.elapsed();
    let stats = db.stats();

    println!();
    println!("{} Build Phase Complete:", "[RESULT]".cyan());
    println!(
        "  SSTables Created: {}",
        stats.sstable_count.to_string().yellow()
    );
    println!(
        "  Total Records: {}",
        stats.total_records.to_string().yellow()
    );
    println!(
        "  Total Size: {:.2} MB",
        (stats.total_size_bytes as f64 / 1024.0 / 1024.0)
            .to_string()
            .yellow()
    );
    println!("  Build Time: {:?}", build_duration);
    println!();


    println!(
        "{} Random Read Test (p50/p95/p99 latency)...",
        "[PHASE 2]".green().bold()
    );

    let mut read_latencies = Vec::with_capacity(ops_per_phase);
    use rand::Rng;
    let mut rng = rand::thread_rng();

    let read_start = Instant::now();
    for _ in 0..ops_per_phase {
        let random_id = rng.gen_range(0..total_records);
        let key = format!("key_{:08}", random_id);

        let op_start = Instant::now();
        let _ = db.get(&key)?;
        read_latencies.push(op_start.elapsed());
    }
    let read_duration = read_start.elapsed();


    read_latencies.sort();
    let p50 = read_latencies[read_latencies.len() * 50 / 100];
    let p95 = read_latencies[read_latencies.len() * 95 / 100];
    let p99 = read_latencies[read_latencies.len() * 99 / 100];
    let max = read_latencies[read_latencies.len() - 1];

    println!();
    println!("{} Read Performance:", "[RESULT]".cyan());
    println!("  Operations: {}", ops_per_phase.to_string().yellow());
    println!("  Duration: {:?}", read_duration);
    println!(
        "  Throughput: {:.0} ops/sec",
        (ops_per_phase as f64 / read_duration.as_secs_f64())
            .to_string()
            .yellow()
    );
    println!();
    println!("  Latency Percentiles:");
    println!("    p50: {:.2} μs", p50.as_micros());
    println!("    p95: {:.2} μs", p95.as_micros());
    println!(
        "    p99: {} μs {}",
        p99.as_micros().to_string().yellow().bold(),
        "← CRITICAL METRIC".dimmed()
    );
    println!("    max: {:.2} μs", max.as_micros());
    println!();


    println!(
        "{} Mixed Workload Test (70% Read / 30% Write)...",
        "[PHASE 3]".green().bold()
    );

    let mut mixed_read_latencies = Vec::new();
    let mut mixed_write_latencies = Vec::new();

    let mixed_start = Instant::now();
    for i in 0..ops_per_phase {
        let is_write = rng.gen_bool(0.3);

        if is_write {
            let key = format!("new_key_{:08}", i);
            let value = format!("new_value_{:08}", i).into_bytes();

            let op_start = Instant::now();
            db.put(key, value)?;
            mixed_write_latencies.push(op_start.elapsed());
        } else {
            let random_id = rng.gen_range(0..total_records);
            let key = format!("key_{:08}", random_id);

            let op_start = Instant::now();
            let _ = db.get(&key)?;
            mixed_read_latencies.push(op_start.elapsed());
        }
    }
    let mixed_duration = mixed_start.elapsed();


    mixed_read_latencies.sort();
    mixed_write_latencies.sort();

    let mixed_r_p99 = mixed_read_latencies[mixed_read_latencies.len() * 99 / 100];
    let mixed_w_p99 = mixed_write_latencies[mixed_write_latencies.len() * 99 / 100];

    println!();
    println!("{} Mixed Workload Performance:", "[RESULT]".cyan());
    println!("  Total Operations: {}", ops_per_phase.to_string().yellow());
    println!(
        "  Reads: {} | Writes: {}",
        mixed_read_latencies.len(),
        mixed_write_latencies.len()
    );
    println!("  Duration: {:?}", mixed_duration);
    println!(
        "  Throughput: {:.0} ops/sec",
        (ops_per_phase as f64 / mixed_duration.as_secs_f64())
            .to_string()
            .yellow()
    );
    println!();
    println!(
        "  Read p99: {} μs",
        mixed_r_p99.as_micros().to_string().yellow()
    );
    println!(
        "  Write p99: {} μs",
        mixed_w_p99.as_micros().to_string().yellow()
    );
    println!();


    let final_stats = db.stats();
    println!("{}", "=".repeat(70).bold());
    println!("{} FINAL DATABASE STATE:", "[SUMMARY]".cyan().bold());
    println!(
        "  SSTables: {}",
        final_stats.sstable_count.to_string().yellow()
    );
    println!(
        "  Total Records: {}",
        final_stats.total_records.to_string().yellow()
    );
    println!(
        "  Total Size: {:.2} MB",
        (final_stats.total_size_bytes as f64 / 1024.0 / 1024.0)
            .to_string()
            .yellow()
    );
    println!(
        "  Cache Entries: {}",
        final_stats.cache_entries.to_string().yellow()
    );
    println!("{}", "=".repeat(70).bold());

    db.close()?;
    Ok(())
}
