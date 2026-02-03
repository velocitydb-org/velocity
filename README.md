# Velocity

A high-performance embedded key-value database built in Rust.

## Features

- Fast read/write operations with LSM-Tree architecture
- Bloom filters for efficient lookups
- Smart caching system
- Write-ahead logging for data safety
- Thread-safe concurrent access
- Optional JSON support

## Quick Start

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

db.close()?;
```

## Configuration

```rust
use velocity::{Velocity, VelocityConfig};

let config = VelocityConfig {
    max_memtable_size: 10_000,
    cache_size: 5_000,
    bloom_false_positive_rate: 0.001,
    compaction_threshold: 8,
    enable_compression: true,
};

let db = Velocity::open_with_config("./db", config)?;
```
x