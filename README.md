# Velocity

A high-performance embedded key-value database built in Rust with LSM-Tree architecture.

## Overview

Velocity is a lightweight, embedded database designed for applications requiring fast read/write operations with minimal overhead. Built with modern storage techniques including LSM-Trees, Bloom filters, and intelligent caching.

## Features

- **LSM-Tree Architecture**: Optimized for write-heavy workloads
- **Bloom Filter**: Reduces unnecessary disk reads
- **Smart LRU Cache**: Intelligent memory management with frequency tracking
- **Write-Ahead Logging**: Ensures data durability and crash recovery
- **Concurrent Access**: Thread-safe operations with Arc/Mutex
- **JSON Support**: Built-in serialization with serde (optional)
- **Compression**: Optional data compression for storage efficiency

## Quick Start

Add Velocity to your `Cargo.toml`:

```toml
[dependencies]
velocity = "0.2.0"
```

### Basic Usage

```rust
use velocity::Velocity;

// open database
let db = Velocity::open("./my_database")?;

// write data
db.put("user:1".to_string(), b"alice".to_vec())?;

// read data
if let Some(value) = db.get("user:1")? {
    println!("user: {}", String::from_utf8_lossy(&value));
}

// close database
db.close()?;
```

### Advanced Configuration

```rust
use velocity::{Velocity, VelocityConfig};

let config = VelocityConfig {
    max_memtable_size: 10_000,
    cache_size: 5_000,
    bloom_false_positive_rate: 0.001,
    compaction_threshold: 8,
    enable_compression: true,
};

let db = Velocity::open_with_config("./optimized_db", config)?;
```

## Architecture

### Storage Engine
- **Memtable**: In-memory sorted data structure
- **SSTable**: Immutable sorted files on disk
- **WAL**: Write-ahead log for durability
- **Compaction**: Background merge operations

### Performance Features
- **Bloom Filters**: Skip unnecessary disk reads
- **LRU Cache**: Keep hot data in memory
- **Sparse Indexing**: Efficient key lookups
- **Batch Operations**: Optimized bulk operations

## API Reference

### Core Operations

```rust
// database management
let db = Velocity::open(path)?;
let db = Velocity::open_with_config(path, config)?;

// data operations
db.put(key: String, value: Vec<u8>)?;
let value = db.get(key: &str)?;

// maintenance
db.flush()?;
db.close()?;

// monitoring
let stats = db.stats();
```

### Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `max_memtable_size` | `usize` | 5000 | Maximum entries in memory |
| `cache_size` | `usize` | 2000 | LRU cache capacity |
| `bloom_false_positive_rate` | `f64` | 0.001 | Bloom filter accuracy |
| `compaction_threshold` | `usize` | 8 | SSTable merge trigger |
| `enable_compression` | `bool` | false | Data compression |

## Performance

Velocity is optimized for:
- **High write throughput**: LSM-Tree design favors writes
- **Efficient reads**: Bloom filters and caching minimize disk I/O
- **Low memory footprint**: Configurable memory usage
- **Fast recovery**: WAL enables quick startup

### Benchmarks

```
write operations: ~100k ops/sec
read operations:  ~80k ops/sec
memory usage:     ~10MB for 1M entries
```

## Examples

See `src/main.rs` for comprehensive usage examples including:
- Basic operations
- JSON storage
- Batch processing
- Concurrent access
- Error handling
- Performance optimization
- Data migration
- Backup and recovery

## Requirements

- Rust 1.70+
- Supported platforms: Linux, macOS, Windows

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome. Please ensure all tests pass and follow the existing code style.

```bash
cargo test
cargo fmt
cargo clippy
```

