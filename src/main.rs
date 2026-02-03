// examples/usage_examples.rs

use velocity::{Velocity, VelocityConfig};

/// ==================== BASIC USAGE ====================
fn basic_usage_example() {
    println!("\n=== basic usage ===");

    // open database
    let db = Velocity::open("./my_database").unwrap();

    // write data
    db.put("user:1".to_string(), b"alice".to_vec()).unwrap();
    db.put("user:2".to_string(), b"bob".to_vec()).unwrap();

    // read data
    if let Some(value) = db.get("user:1").unwrap() {
        let name = String::from_utf8_lossy(&value);
        println!("user 1: {}", name);
    }

    // display statistics
    let stats = db.stats();
    println!("memtable: {} entries", stats.memtable_entries);
    println!("sstable: {} files", stats.sstable_count);

    // close database (automatic cleanup)
    db.close().unwrap();
}

/// ==================== ADVANCED CONFIGURATION ====================
fn advanced_configuration_example() {
    println!("\n=== advanced configuration ===");

    let config = VelocityConfig {
        max_memtable_size: 5000,
        cache_size: 1000,
        bloom_false_positive_rate: 0.001,
        compaction_threshold: 8,
        enable_compression: false,
    };

    let _db = Velocity::open_with_config("./optimized_db", config).unwrap();

    println!("started with optimized configuration");
}

/// ==================== JSON STORAGE ====================
fn json_storage_example() {
    println!("\n=== json storage ===");

    use serde::{Serialize, Deserialize};

    #[derive(Serialize, Deserialize, Debug)]
    struct User {
        id: u32,
        name: String,
        email: String,
        age: u8,
    }

    let db = Velocity::open("./json_db").unwrap();

    // save as json
    let user = User {
        id: 1,
        name: "alice".to_string(),
        email: "alice@example.com".to_string(),
        age: 30,
    };

    let json = serde_json::to_vec(&user).unwrap();
    db.put(format!("user:{}", user.id), json).unwrap();

    // read as json
    if let Some(data) = db.get("user:1").unwrap() {
        let loaded_user: User = serde_json::from_slice(&data).unwrap();
        println!("loaded user: {:?}", loaded_user);
    }
}

/// ==================== BATCH OPERATIONS ====================
fn batch_operations_example() {
    println!("\n=== batch operations ===");

    let db = Velocity::open("./batch_db").unwrap();

    // bulk write
    let start = std::time::Instant::now();
    for i in 0..10_000 {
        let key = format!("item:{}", i);
        let value = format!("data_{}", i).into_bytes();
        db.put(key, value).unwrap();
    }
    let duration = start.elapsed();

    println!("wrote 10,000 entries in {:?}", duration);

    // bulk read
    let start = std::time::Instant::now();
    for i in 0..10_000 {
        let key = format!("item:{}", i);
        let _ = db.get(&key).unwrap();
    }
    let duration = start.elapsed();

    println!("read 10,000 entries in {:?}", duration);
}

/// ==================== KEY-VALUE PATTERNS ====================
fn key_patterns_example() {
    println!("\n=== key-value patterns ===");

    let db = Velocity::open("./pattern_db").unwrap();

    // pattern 1: namespaced keys
    db.put("user:1:profile".to_string(), b"alice profile".to_vec()).unwrap();
    db.put("user:1:settings".to_string(), b"dark mode".to_vec()).unwrap();
    db.put("user:2:profile".to_string(), b"bob profile".to_vec()).unwrap();

    // pattern 2: timestamped keys
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    db.put(
        format!("event:{}:login", timestamp),
        b"user logged in".to_vec()
    ).unwrap();

    // pattern 3: composite keys
    db.put("session:abc123:user:1".to_string(), b"session data".to_vec()).unwrap();

    println!("various key patterns applied");
}

/// ==================== ERROR HANDLING ====================
fn error_handling_example() {
    println!("\n=== error handling ===");

    let db = Velocity::open("./error_db").unwrap();

    // safe read
    match db.get("unknown_key") {
        Ok(Some(value)) => {
            println!("value found: {:?}", value);
        }
        Ok(None) => {
            println!("key not found");
        }
        Err(e) => {
            eprintln!("error occurred: {:?}", e);
        }
    }

    // safe write
    if let Err(e) = db.put("key".to_string(), vec![1, 2, 3]) {
        eprintln!("write error: {:?}", e);
    } else {
        println!("write successful");
    }
}

/// ==================== PERFORMANCE OPTIMIZATION ====================
fn performance_optimization_example() {
    println!("\n=== performance optimization ===");

    // optimized config for large cache
    let config = VelocityConfig {
        max_memtable_size: 10_000,
        cache_size: 5_000,
        bloom_false_positive_rate: 0.001,
        compaction_threshold: 10,
        enable_compression: true,      // compression enabled for performance
    };

    let db = Velocity::open_with_config("./perf_db", config).unwrap();

    // write frequently accessed data first (load into cache)
    db.put("hot_key_1".to_string(), b"frequently accessed".to_vec()).unwrap();
    db.put("hot_key_2".to_string(), b"frequently accessed".to_vec()).unwrap();

    // manual flush control for batch operations
    for i in 0..5_000 {
        db.put(format!("batch:{}", i), vec![i as u8]).unwrap();
    }

    // manual flush (optional)
    db.flush().unwrap();

    println!("performance optimizations applied");
}

/// ==================== CONCURRENT USAGE ====================
fn concurrent_usage_example() {
    println!("\n=== concurrent usage ===");

    use std::sync::Arc;
    use std::thread;

    let db = Arc::new(Velocity::open("./concurrent_db").unwrap());
    let mut handles = vec![];

    // multi-threaded writes
    for thread_id in 0..4 {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            for i in 0..1_000 {
                let key = format!("thread_{}:item_{}", thread_id, i);
                let value = format!("data from thread {}", thread_id).into_bytes();
                db_clone.put(key, value).unwrap();
            }
        });
        handles.push(handle);
    }

    // wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    println!("wrote 4,000 entries from 4 threads");

    // display statistics
    let stats = db.stats();
    println!("total entries: {}", stats.memtable_entries);
}

/// ==================== DATA MIGRATION ====================
fn data_migration_example() {
    println!("\n=== data migration ===");

    let old_db = Velocity::open("./old_db").unwrap();
    let new_db = Velocity::open("./new_db").unwrap();

    // sample data in old database
    for i in 0..1_000 {
        old_db.put(format!("old:{}", i), vec![i as u8]).unwrap();
    }

    // migration (in real scenarios you would iterate over all keys)
    println!("migrating data...");
    for i in 0..1_000 {
        if let Some(value) = old_db.get(&format!("old:{}", i)).unwrap() {
            new_db.put(format!("new:{}", i), value).unwrap();
        }
    }

    println!("migration completed");
}

/// ==================== BACKUP & RECOVERY ====================
fn backup_recovery_example() {
    println!("\n=== backup & recovery ===");

    // production database
    let db = Velocity::open("./production_db").unwrap();
    db.put("critical_data".to_string(), b"important value".to_vec()).unwrap();
    db.flush().unwrap(); // write to disk

    // backup (filesystem level)
    use std::fs;
    println!("creating backup...");
    let _ = fs::copy("./production_db/velocity.db", "./backup_db/velocity.db");

    // recovery test
    drop(db); // close database

    println!("performing recovery...");
    let recovered_db = Velocity::open("./production_db").unwrap();

    if let Some(value) = recovered_db.get("critical_data").unwrap() {
        println!("data recovered: {:?}", String::from_utf8_lossy(&value));
    }
}

/// ==================== MONITORING ====================
fn monitoring_example() {
    println!("\n=== monitoring ===");

    let db = Velocity::open("./monitored_db").unwrap();

    // perform some operations
    for i in 0..1_000 {
        db.put(format!("key:{}", i), vec![i as u8]).unwrap();
    }

    // collect statistics
    let stats = db.stats();

    println!("\ndatabase statistics:");
    println!("  ├─ memtable entries: {}", stats.memtable_entries);
    println!("  ├─ sstable files: {}", stats.sstable_count);
    println!("  ├─ cache entries: {}", stats.cache_entries);
    println!("  └─ total sstable size: {} bytes", stats.total_sstable_size);

    // estimated ram usage
    let estimated_ram = stats.memtable_entries * 100; // average entry size
    println!("\nestimated ram usage: {} kb", estimated_ram / 1024);
}

/// ==================== MAIN PROGRAM ====================
fn main() {
    println!("velocity database - usage examples\n");

    basic_usage_example();
    advanced_configuration_example();
    json_storage_example();
    batch_operations_example();
    key_patterns_example();
    error_handling_example();
    performance_optimization_example();
    concurrent_usage_example();
    data_migration_example();
    backup_recovery_example();
    monitoring_example();

    println!("\nall examples completed");
}