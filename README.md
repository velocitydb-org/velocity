# Velocity Database

Velocity is a high-performance, distributed key-value storage engine engineered in Rust. It is designed for low-latency data ingestion and high-concurrency read operations, leveraging a custom binary protocol and an optimized LSM-Tree architecture.

## Architecture Overview

The system is built upon a Log-Structured Merge-Tree (LSM-Tree) architecture, ensuring high throughput for write-intensive workloads while maintaining efficient read performance through tiered storage and advanced indexing.

### Core Components

*   **LSM-Tree Storage**: Optimized persistence layer with tiered SSTables.
*   **Memtable**: In-memory write buffer for sub-microsecond ingestion.
*   **Write-Ahead Log (WAL)**: Ensures data durability and crash recovery.
*   **Bloom Filters**: Probabilistic data structures to minimize unnecessary disk I/O.
*   **Velocity Protocol**: A custom binary protocol designed for minimal overhead and maximum security.

## Velocity Protocol Specification

The Velocity Protocol (V-Proto) is a proprietary binary specification for secure communication over TCP/TLS.

### Message Structure
Messages are serialized in little-endian format with the following header:
*   **Magic Number (4 bytes)**: 0x56454C4F ("VELO")
*   **Version (1 byte)**: Protocol version identifier.
*   **Message Type (1 byte)**: Command or response classification.
*   **Payload Length (4 bytes)**: Size of the following data segment.
*   **Payload (Variable)**: Command-specific data.
*   **Checksum (4 bytes)**: Integrity verification segment.

### Authentication
The protocol implements a secure handshake incorporating server fingerprint verification and Argon2id-hashed credential transmission, preventing man-in-the-middle attacks and ensuring credential safety.

## Operational Performance

Velocity is optimized for enterprise-scale performance:
*   **Read Latency**: Sub-millisecond response times for cached data.
*   **Write Throughput**: Engineered to exceed 100,000 operations per second.
*   **Security**: TLS 1.3 transport encryption and per-user rate limiting.

## Deployment and Usage

The server can be initialized via the command-line interface:

```bash
# Initialize storage engine
cargo run -- server --bind 127.0.0.1:2005 --data-dir ./data

# Provision administrative user
cargo run -- create-user --username admin --password [secure_password]
```

### SDK Integration

The official Rust SDK provides a thread-safe connection pool for high-concurrency applications:

```rust
use velocity::client::VelocityPool;

let pool = VelocityPool::new(
    "127.0.0.1:2005".to_string(),
    "username".to_string(),
    "password".to_string(),
    20 
);

let mut connection = pool.get_connection().await?;
connection.insert("key", "value").await?;
```

## Governance and License

This project is licensed under the MIT License. For further information or enterprise support, please refer to the official documentation.

## Operational Observability & Reliability

### Monitoring
Velocity ships with the Studio operational console (`src/studio.rs`), which exposes `/api/analysis` for configuration and sanity checks plus `/api/stats` for aggregate `VelocityStats`. Studio will launch on the bound address (e.g., `http://127.0.0.1:2005` if you call `cargo run -- studio`) and highlights risks such as missing `velocity.toml` settings, disabled backup addons, and SSTable pressure so you can alert on those conditions from your monitoring stack.

### Metrics
Low-level instrumentation lives in `src/performance.rs`. `PerformanceMetrics` counts reads/writes, cache hits/misses, errors/timeouts, and records latency percentiles; the adaptive cache manager consults that data to tune cache sizing automatically. Enable the collector in `velocity.toml` under `[performance]` (`enable_metrics = true`, `metrics_interval = 60` seconds, `target_cache_hit_rate`) to emit snapshots, and wire those snapshots into whatever exporter you prefer.

### Backup strategy
Velocity exposes a backup addon (`crate::addon::BackupAddonConfig`) that can be enabled via `velocity.toml` under `[addons.backup]`. Configure `backup_path`, `interval_minutes`, and whether to snapshot every managed database (or a whitelist via `target_databases`). When the addon is active the manager periodically calls `backup_all_databases()` to copy each database directory into timestamped subdirectories; you can also trigger the same logic from the Studio interface or CLI commands for on-demand restores.

### Upgrade story
To upgrade, drain traffic, stop the running binary, `git pull` the latest changes, and rebuild with the Makefile or Cargo: `make release` / `cargo build --release` (or `cargo install --path .` for systems installs). The Makefile already packages `velocity.toml`, the README, and the stored binary, and there are `docker`/`docker-compose` recipes for containerized rollouts. Once the new binary is in place, restart the server against the existing data directory; WAL replay and SSTable compaction will bring nodes up to date without extra migrations.

### Corruption detection
Every WAL entry records an 8-byte checksum computed by `Velocity::calculate_checksum`; recovery (`wal::recover`) replays only entries whose stored checksum matches the recomputed hash, so transient corruptions are dropped before they affect the LSM. SSTables and Bloom filters are similarly guarded by the underlying crate (`src/lib.rs`), and the Studio analysis step warns if any configured path is missing or exhibits an unexpected SSTable count. Combine these safeguards with the backup addon so you have safe fallbacks when corruption is detected.
