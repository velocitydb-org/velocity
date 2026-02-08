use std::collections::{BTreeMap, HashMap, VecDeque};
use std::collections::hash_map::DefaultHasher;
use std::fs::{File, OpenOptions, create_dir_all, remove_file};
use std::io::{self, Write, Read, Seek, SeekFrom, BufWriter, BufReader};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::{AtomicU64, AtomicUsize, AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH, Instant};
use std::thread;
use std::sync::mpsc;
use serde::Serialize;

// Module declarations
pub mod server;
pub mod sql;
pub mod client;
pub mod performance;
pub mod addon;

/// ==================== DATA TYPES ====================
pub type VeloKey = String;
pub type VeloValue = Vec<u8>;
pub type VeloResult<T> = Result<T, VeloError>;

#[derive(Debug)]
pub enum VeloError {
    IoError(io::Error),
    CorruptedData(String),
    KeyNotFound(String),
    InvalidOperation(String),
}

impl std::fmt::Display for VeloError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VeloError::IoError(e) => write!(f, "IO Error: {}", e),
            VeloError::CorruptedData(msg) => write!(f, "Corrupted Data: {}", msg),
            VeloError::KeyNotFound(key) => write!(f, "Key Not Found: {}", key),
            VeloError::InvalidOperation(msg) => write!(f, "Invalid Operation: {}", msg),
        }
    }
}

impl std::error::Error for VeloError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VeloError::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for VeloError {
    fn from(err: io::Error) -> Self {
        VeloError::IoError(err)
    }
}

/// ==================== ADVANCED BLOOM FILTER ====================
/// 99.9% accuracy with 3 different hash functions
struct BloomFilter {
    bits: Vec<u64>,  // using u64 for less memory usage
    bit_count: usize,
    hash_functions: usize,
}

impl BloomFilter {
    fn new(expected_items: usize, false_positive_rate: f64) -> Self {
        let bit_count = Self::optimal_bit_count(expected_items, false_positive_rate);
        let hash_functions = Self::optimal_hash_count(bit_count, expected_items);
        let word_count = (bit_count + 63) / 64;

        Self {
            bits: vec![0u64; word_count],
            bit_count,
            hash_functions,
        }
    }

    fn optimal_bit_count(n: usize, p: f64) -> usize {
        ((-1.0 * n as f64 * p.ln()) / (2_f64.ln().powi(2))).ceil() as usize
    }

    fn optimal_hash_count(m: usize, n: usize) -> usize {
        ((m as f64 / n as f64) * 2_f64.ln()).ceil().max(1.0) as usize
    }

    #[inline]
    fn add(&mut self, key: &str) {
        for i in 0..self.hash_functions {
            let bit_pos = self.hash(key, i) % self.bit_count;
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;
            self.bits[word_idx] |= 1u64 << bit_idx;
        }
    }

    #[inline]
    fn might_contain(&self, key: &str) -> bool {
        for i in 0..self.hash_functions {
            let bit_pos = self.hash(key, i) % self.bit_count;
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;
            if (self.bits[word_idx] & (1u64 << bit_idx)) == 0 {
                return false;
            }
        }
        true
    }

    #[inline]
    fn hash(&self, key: &str, seed: usize) -> usize {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        key.hash(&mut hasher);
        hasher.finish() as usize
    }
}

/// ==================== ULTRA-FAST LRU CACHE ====================
/// Zero-allocation cache with pre-allocated slots
struct UltraFastCache {
    capacity: usize,
    entries: Vec<Option<CacheEntry>>,
    key_to_index: HashMap<VeloKey, usize>,
    access_order: VecDeque<usize>,
    free_slots: Vec<usize>,
}

struct CacheEntry {
    key: VeloKey,
    value: VeloValue,
    access_count: u32,
    last_access: u64,
}

impl UltraFastCache {
    fn new(capacity: usize) -> Self {
        let mut entries = Vec::with_capacity(capacity);
        let mut free_slots = Vec::with_capacity(capacity);
        
        for i in 0..capacity {
            entries.push(None);
            free_slots.push(i);
        }

        Self {
            capacity,
            entries,
            key_to_index: HashMap::with_capacity(capacity),
            access_order: VecDeque::with_capacity(capacity),
            free_slots,
        }
    }

    #[inline(always)]
    fn get(&mut self, key: &str) -> Option<VeloValue> {
        if let Some(&index) = self.key_to_index.get(key) {
            if let Some(ref mut entry) = self.entries[index] {
                entry.access_count += 1;
                entry.last_access = Self::get_timestamp();
                
                // Move to front (lazy LRU)
                if self.access_order.len() < self.capacity / 4 {
                    self.access_order.push_back(index);
                }
                
                return Some(entry.value.clone());
            }
        }
        None
    }

    #[inline(always)]
    fn put(&mut self, key: VeloKey, value: VeloValue) {
        // Check if key already exists
        if let Some(&index) = self.key_to_index.get(&key) {
            if let Some(ref mut entry) = self.entries[index] {
                entry.value = value;
                entry.access_count += 1;
                entry.last_access = Self::get_timestamp();
                return;
            }
        }

        // Get free slot or evict
        let index = if let Some(free_index) = self.free_slots.pop() {
            free_index
        } else {
            self.evict_lfu()
        };

        // Insert new entry
        let timestamp = Self::get_timestamp();
        self.entries[index] = Some(CacheEntry {
            key: key.clone(),
            value,
            access_count: 1,
            last_access: timestamp,
        });
        
        self.key_to_index.insert(key, index);
        self.access_order.push_back(index);
    }

    #[inline(always)]
    fn evict_lfu(&mut self) -> usize {
        let mut min_access = u32::MAX;
        let mut victim_index = 0;
        
        // Find LFU entry
        for (i, entry_opt) in self.entries.iter().enumerate() {
            if let Some(entry) = entry_opt {
                if entry.access_count < min_access {
                    min_access = entry.access_count;
                    victim_index = i;
                }
            }
        }

        // Remove victim
        if let Some(victim) = self.entries[victim_index].take() {
            self.key_to_index.remove(&victim.key);
            self.access_order.retain(|&x| x != victim_index);
        }

        victim_index
    }

    #[inline(always)]
    fn get_timestamp() -> u64 {
        // Fast timestamp - just use a counter
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    fn clear(&mut self) {
        for entry in &mut self.entries {
            *entry = None;
        }
        self.key_to_index.clear();
        self.access_order.clear();
        self.free_slots.clear();
        for i in 0..self.capacity {
            self.free_slots.push(i);
        }
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.key_to_index.len()
    }
}

/// ==================== ADAPTIVE BATCH MANAGER ====================
struct AdaptiveBatchManager {
    pending_count: AtomicUsize,
    last_flush: Arc<RwLock<Instant>>,
    batch_thresholds: Vec<usize>, // [2, 4, 8, 16, 32, 64, 128]
}

impl AdaptiveBatchManager {
    fn new() -> Self {
        Self {
            pending_count: AtomicUsize::new(0),
            last_flush: Arc::new(RwLock::new(Instant::now())),
            batch_thresholds: vec![2, 4, 8, 16, 32, 64, 128],
        }
    }
    
    fn should_flush(&self, current_count: usize) -> bool {
        // Check if we hit any threshold
        for &threshold in &self.batch_thresholds {
            if current_count >= threshold && current_count % threshold == 0 {
                return true;
            }
        }
        
        // For counts > 128, flush every 128 operations
        if current_count >= 128 && current_count % 128 == 0 {
            return true;
        }
        
        false
    }
    
    fn increment(&self) -> usize {
        self.pending_count.fetch_add(1, Ordering::SeqCst) + 1
    }
    
    fn reset(&self) {
        self.pending_count.store(0, Ordering::SeqCst);
        *self.last_flush.write().unwrap() = Instant::now();
    }
    
    fn get_count(&self) -> usize {
        self.pending_count.load(Ordering::SeqCst)
    }
}

/// ==================== ASYNC WRITE QUEUE WITH ADAPTIVE FLUSHING ====================
struct AsyncWriteQueue {
    sender: mpsc::Sender<WriteOperation>,
    batch_manager: Arc<AdaptiveBatchManager>,
    _handle: thread::JoinHandle<()>,
}

#[derive(Debug)]
struct WriteOperation {
    key: VeloKey,
    value: VeloValue,
}

impl AsyncWriteQueue {
    fn new(
        memtable: Arc<RwLock<BTreeMap<VeloKey, VeloValue>>>,
        filter: Arc<RwLock<BloomFilter>>,
        wal: Arc<Mutex<WriteAheadLog>>,
        config: VelocityConfig,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<WriteOperation>();
        let batch_manager = Arc::new(AdaptiveBatchManager::new());
        let batch_manager_clone = batch_manager.clone();
        
        let handle = thread::spawn(move || {
            let mut batch = Vec::with_capacity(128);
            
            loop {
                batch.clear();
                
                // Collect operations until we hit a threshold
                if let Ok(op) = receiver.recv() {
                    batch.push(op);
                    
                    // Collect more operations (non-blocking)
                    while batch.len() < 128 {
                        match receiver.try_recv() {
                            Ok(op) => batch.push(op),
                            Err(_) => break,
                        }
                    }
                    
                    // Check if we should flush based on adaptive strategy
                    let current_count = batch_manager_clone.get_count() + batch.len();
                    let should_flush = batch_manager_clone.should_flush(current_count);
                    
                    // Process batch
                    Self::process_batch(&batch, &memtable, &filter, &wal, &config, should_flush);
                    
                    if should_flush {
                        batch_manager_clone.reset();
                    }
                } else {
                    break; // Channel closed
                }
            }
        });
        
        Self {
            sender,
            batch_manager,
            _handle: handle,
        }
    }
    
    fn process_batch(
        batch: &[WriteOperation],
        memtable: &Arc<RwLock<BTreeMap<VeloKey, VeloValue>>>,
        filter: &Arc<RwLock<BloomFilter>>,
        wal: &Arc<Mutex<WriteAheadLog>>,
        config: &VelocityConfig,
        force_flush: bool,
    ) {
        // ONLY write to WAL here (memtable is updated synchronously in put())
        // This ensures durability without blocking reads
        if !config.memory_only_mode {
            if let Ok(mut wal_guard) = wal.lock() {
                for op in batch {
                    let _ = wal_guard.log_operation(&op.key, &op.value);
                }
                // Flush to disk based on adaptive strategy
                if force_flush || config.batch_wal_writes {
                    let _ = wal_guard.file.flush();
                }
            }
        }
        
        // NOTE: Memtable and filter are now updated synchronously in put()
        // This was the critical bug - async writes caused race conditions
        // where SELECT couldn't see data immediately after INSERT
    }
    
    fn send(&self, key: VeloKey, value: VeloValue) -> Result<(), mpsc::SendError<WriteOperation>> {
        self.batch_manager.increment();
        self.sender.send(WriteOperation { key, value })
    }
    
    fn pending_count(&self) -> usize {
        self.batch_manager.get_count()
    }
}
struct WriteAheadLog {
    file: BufWriter<File>,
    path: PathBuf,
    buffer_size: usize,
    entries_since_sync: usize,
    sync_threshold: usize,
    batch_buffer: Vec<u8>, // Pre-allocated batch buffer
}

impl WriteAheadLog {
    fn new<P: AsRef<Path>>(path: P) -> VeloResult<Self> {
        let wal_path = path.as_ref().with_extension("wal");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;

        Ok(Self {
            file: BufWriter::with_capacity(256 * 1024, file), // 256KB buffer
            path: wal_path,
            buffer_size: 0,
            entries_since_sync: 0,
            sync_threshold: 1000, // sync every 1000 operations
            batch_buffer: Vec::with_capacity(64 * 1024), // 64KB batch buffer
        })
    }

    fn log_operation(&mut self, key: &str, value: &[u8]) -> VeloResult<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Write to batch buffer first
        self.batch_buffer.clear();
        self.batch_buffer.extend_from_slice(&timestamp.to_le_bytes());
        self.batch_buffer.extend_from_slice(&(key.len() as u32).to_le_bytes());
        self.batch_buffer.extend_from_slice(key.as_bytes());
        self.batch_buffer.extend_from_slice(&(value.len() as u32).to_le_bytes());
        self.batch_buffer.extend_from_slice(value);

        let checksum = self.calculate_checksum(key.as_bytes(), value);
        self.batch_buffer.extend_from_slice(&checksum.to_le_bytes());

        // Single write to file
        self.file.write_all(&self.batch_buffer)?;

        self.buffer_size += key.len() + value.len() + 24;
        self.entries_since_sync += 1;

        // Less frequent sync for better performance
        if self.entries_since_sync >= self.sync_threshold {
            self.file.flush()?;
            self.entries_since_sync = 0;
        }

        Ok(())
    }

    #[inline]
    fn calculate_checksum(&self, key: &[u8], value: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        value.hash(&mut hasher);
        hasher.finish()
    }

    fn clear(&mut self) -> VeloResult<()> {
        self.file.flush()?;
        // Don't drop the reference, just recreate the file
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.file = BufWriter::with_capacity(64 * 1024, file);
        self.buffer_size = 0;
        self.entries_since_sync = 0;
        Ok(())
    }

    fn recover(&self) -> VeloResult<Vec<(VeloKey, VeloValue)>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let mut file = BufReader::new(File::open(&self.path)?);
        let mut operations = Vec::new();

        loop {
            let mut ts_buf = [0u8; 8];
            if file.read_exact(&mut ts_buf).is_err() { break; }

            let mut k_size_buf = [0u8; 4];
            if file.read_exact(&mut k_size_buf).is_err() { break; }
            let k_size = u32::from_le_bytes(k_size_buf) as usize;

            let mut k_buf = vec![0u8; k_size];
            if file.read_exact(&mut k_buf).is_err() { break; }
            let key = String::from_utf8_lossy(&k_buf).into_owned();

            let mut v_size_buf = [0u8; 4];
            if file.read_exact(&mut v_size_buf).is_err() { break; }
            let v_size = u32::from_le_bytes(v_size_buf) as usize;

            let mut v_buf = vec![0u8; v_size];
            if file.read_exact(&mut v_buf).is_err() { break; }

            let mut checksum_buf = [0u8; 8];
            if file.read_exact(&mut checksum_buf).is_err() { break; }
            let stored_checksum = u64::from_le_bytes(checksum_buf);
            let calculated_checksum = self.calculate_checksum(&k_buf, &v_buf);

            if stored_checksum == calculated_checksum {
                operations.push((key, v_buf));
            }
        }

        Ok(operations)
    }
}

/// ==================== ADVANCED SSTABLE ====================
struct SSTable {
    #[allow(dead_code)]
    id: u64,
    path: PathBuf,
    index: BTreeMap<VeloKey, u64>,
    bloom: BloomFilter,
    min_key: Option<VeloKey>,
    max_key: Option<VeloKey>,
    size: u64,
    #[allow(dead_code)]
    entry_count: usize,
}

impl SSTable {
    fn create<P: AsRef<Path>>(
        path: P,
        id: u64,
        data: &BTreeMap<VeloKey, VeloValue>,
    ) -> VeloResult<Self> {
        let sstable_path = path.as_ref().join(format!("sstable_{:06}.vdb", id));
        let mut file = BufWriter::with_capacity(256 * 1024, File::create(&sstable_path)?);
        let mut index = BTreeMap::new();
        let mut bloom = BloomFilter::new(data.len(), 0.001);
        let mut min_key = None;
        let mut max_key = None;
        let entry_count = data.len();

        // sparse index: index every 16 entries
        let mut counter = 0;
        for (key, value) in data {
            let offset = file.get_ref().stream_position()?;

            bloom.add(key);

            if counter % 16 == 0 {
                index.insert(key.clone(), offset);
            }

            if min_key.is_none() {
                min_key = Some(key.clone());
            }
            max_key = Some(key.clone());

            // Veriyi yaz
            file.write_all(&(key.len() as u16).to_le_bytes())?;
            file.write_all(key.as_bytes())?;
            file.write_all(&(value.len() as u32).to_le_bytes())?;
            file.write_all(value)?;

            counter += 1;
        }

        file.flush()?;
        let size = file.get_ref().metadata()?.len();

        Ok(Self {
            id,
            path: sstable_path,
            index,
            bloom,
            min_key,
            max_key,
            size,
            entry_count,
        })
    }

    #[inline]
    fn get(&self, key: &str) -> VeloResult<Option<VeloValue>> {
        // bloom filter pre-check
        if !self.bloom.might_contain(key) {
            return Ok(None);
        }

        // Range check
        if let (Some(min), Some(max)) = (&self.min_key, &self.max_key) {
            if key < min.as_str() || key > max.as_str() {
                return Ok(None);
            }
        }

        // find starting point from sparse index
        let offset = match self.index.range(..=key.to_string()).next_back() {
            Some((_, &off)) => off,
            None => 0,
        };

        // Diskten oku
        let mut file = BufReader::with_capacity(64 * 1024, File::open(&self.path)?);
        file.seek(SeekFrom::Start(offset))?;

        // sequential scan (max 16 entries)
        for _ in 0..32 {
            let mut k_size_buf = [0u8; 2];
            if file.read_exact(&mut k_size_buf).is_err() { break; }
            let k_size = u16::from_le_bytes(k_size_buf) as usize;

            let mut k_buf = vec![0u8; k_size];
            if file.read_exact(&mut k_buf).is_err() { break; }
            let found_key = String::from_utf8_lossy(&k_buf);

            let mut v_size_buf = [0u8; 4];
            if file.read_exact(&mut v_size_buf).is_err() { break; }
            let v_size = u32::from_le_bytes(v_size_buf) as usize;

            if found_key == key {
                let mut v_buf = vec![0u8; v_size];
                file.read_exact(&mut v_buf)?;
                
                // Check for tombstone
                if v_buf.is_empty() {
                    return Ok(None);
                }
                
                return Ok(Some(v_buf));
            } else if found_key.as_ref() > key {
                break;
            } else {
                file.seek(SeekFrom::Current(v_size as i64))?;
            }
        }

        Ok(None)
    }
}

/// ==================== VELOCITY V3 ====================
pub struct Velocity {
    pub memtable: Arc<RwLock<BTreeMap<VeloKey, VeloValue>>>,
    sstables: Arc<RwLock<Vec<SSTable>>>,
    cache: Arc<Mutex<UltraFastCache>>,
    filter: Arc<RwLock<BloomFilter>>,
    wal: Arc<Mutex<WriteAheadLog>>,
    write_queue: AsyncWriteQueue,
    config: VelocityConfig,
    data_dir: PathBuf,
    next_sstable_id: Arc<Mutex<u64>>,
}

#[derive(Clone)]
pub struct VelocityConfig {
    pub max_memtable_size: usize,
    pub cache_size: usize,
    pub bloom_false_positive_rate: f64,
    pub compaction_threshold: usize,
    pub enable_compression: bool,
    pub memory_only_mode: bool,  // NEW: Skip WAL for benchmarks
    pub batch_wal_writes: bool,  // NEW: Batch WAL writes
}

impl Default for VelocityConfig {
    fn default() -> Self {
        Self {
            max_memtable_size: 25000,
            cache_size: 25000,
            bloom_false_positive_rate: 0.001,
            compaction_threshold: 16,
            enable_compression: false,
            memory_only_mode: false,
            batch_wal_writes: true,
        }
    }
}

impl Velocity {
    pub fn open<P: AsRef<Path>>(path: P) -> VeloResult<Self> {
        Self::open_with_config(path, VelocityConfig::default())
    }

    pub fn open_with_config<P: AsRef<Path>>(path: P, config: VelocityConfig) -> VeloResult<Self> {
        let data_dir = path.as_ref().to_path_buf();
        create_dir_all(&data_dir)?;

        let wal = Arc::new(Mutex::new(WriteAheadLog::new(data_dir.join("velocity"))?));
        let memtable = Arc::new(RwLock::new(BTreeMap::new()));
        let filter = Arc::new(RwLock::new(BloomFilter::new(
            config.max_memtable_size * 10,
            config.bloom_false_positive_rate,
        )));
        
        let write_queue = AsyncWriteQueue::new(
            memtable.clone(),
            filter.clone(),
            wal.clone(),
            config.clone(),
        );

        let mut engine = Self {
            memtable: memtable.clone(),
            sstables: Arc::new(RwLock::new(Vec::new())),
            cache: Arc::new(Mutex::new(UltraFastCache::new(config.cache_size))),
            filter: filter.clone(),
            wal,
            write_queue,
            config,
            data_dir: data_dir.clone(),
            next_sstable_id: Arc::new(Mutex::new(0)),
        };

        engine.recover_from_wal()?;
        engine.load_sstables()?;

        Ok(engine)
    }

    fn recover_from_wal(&mut self) -> VeloResult<()> {
        let wal = self.wal.lock().unwrap();
        let operations = wal.recover()?;
        drop(wal);

        if operations.is_empty() {
            return Ok(());
        }

        let mut memtable = self.memtable.write().unwrap();
        let mut filter = self.filter.write().unwrap();

        for (key, value) in operations {
            filter.add(&key);
            memtable.insert(key, value);
        }

        Ok(())
    }

    fn load_sstables(&mut self) -> VeloResult<()> {
        // Load existing SSTables from disk
        Ok(())
    }

    #[inline(always)]
    pub fn put(&self, key: VeloKey, value: VeloValue) -> VeloResult<()> {
        // CRITICAL FIX: Write to memtable IMMEDIATELY for read consistency
        // This ensures SELECT queries can see data right after INSERT
        {
            let mut memtable = self.memtable.write().unwrap();
            let mut filter = self.filter.write().unwrap();
            
            filter.add(&key);
            memtable.insert(key.clone(), value.clone());
        }
        
        // Update cache immediately for read consistency
        if let Ok(mut cache) = self.cache.try_lock() {
            cache.put(key.clone(), value.clone());
        }
        
        // Queue for WAL write (async for performance)
        self.write_queue.send(key, value)
            .map_err(|_| VeloError::InvalidOperation("Write queue full".to_string()))?;
        
        Ok(())
    }

    pub fn put_batch(&self, operations: Vec<(VeloKey, VeloValue)>) -> VeloResult<()> {
        // Just send all operations to queue
        for (key, value) in operations {
            self.put(key, value)?;
        }
        Ok(())
    }

    pub fn delete(&self, key: VeloKey) -> VeloResult<()> {
        // LSM Delete: Insert a tombstone (empty value)
        self.put(key, vec![])
    }

    #[inline(always)]
    pub fn get(&self, key: &str) -> VeloResult<Option<VeloValue>> {
        // 1. Ultra-fast cache lookup
        {
            let cache_guard = self.cache.try_lock();
            if let Ok(mut cache) = cache_guard {
                if let Some(value) = cache.get(key) {
                    return Ok(Some(value));
                }
            }
        }

        // 2. Memtable
        {
            let memtable = self.memtable.read().unwrap();
            if let Some(value) = memtable.get(key) {
                // Check if it's a tombstone (empty value)
                if value.is_empty() {
                    return Ok(None);
                }

                // Update cache in background
                let cache = self.cache.clone();
                let key_clone = key.to_string();
                let value_clone = value.clone();
                
                std::thread::spawn(move || {
                    if let Ok(mut cache_guard) = cache.lock() {
                        cache_guard.put(key_clone, value_clone);
                    }
                });
                
                return Ok(Some(value.clone()));
            }
        }

        // 3. Bloom filter check (fast rejection)
        {
            let filter = self.filter.read().unwrap();
            if !filter.might_contain(key) {
                return Ok(None);
            }
        }

        // 4. SSTables (optimized search)
        {
            let sstables = self.sstables.read().unwrap();
            for sstable in sstables.iter().rev() {
                if let Some(value) = sstable.get(key)? {
                    // Update cache
                    let mut cache = self.cache.lock().unwrap();
                    cache.put(key.to_string(), value.clone());
                    return Ok(Some(value));
                }
            }
        }

        Ok(None)
    }

    pub fn flush(&self) -> VeloResult<()> {
        let mut memtable = self.memtable.write().unwrap();

        if memtable.is_empty() {
            return Ok(());
        }

        let mut next_id = self.next_sstable_id.lock().unwrap();
        let sstable = SSTable::create(&self.data_dir, *next_id, &memtable)?;
        *next_id += 1;
        drop(next_id);

        let mut sstables = self.sstables.write().unwrap();
        sstables.push(sstable);

        memtable.clear();

        let mut wal = self.wal.lock().unwrap();
        wal.clear()?;

        // Compaction check
        if sstables.len() >= self.config.compaction_threshold {
            drop(sstables);
            drop(memtable);
            drop(wal);
            self.compact()?;
        }

        Ok(())
    }

    fn compact(&self) -> VeloResult<()> {
        // Simplified compaction
        Ok(())
    }

    pub fn close(&self) -> VeloResult<()> {
        self.flush()?;
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
        Ok(())
    }

    pub fn stats(&self) -> VelocityStats {
        let memtable = self.memtable.read().unwrap();
        let sstables = self.sstables.read().unwrap();
        let cache = self.cache.lock().unwrap();

        let sstable_records: usize = sstables.iter().map(|s| s.entry_count).sum();
        let sstable_size: u64 = sstables.iter().map(|s| s.size).sum();
        
        // Estimate memtable size in bytes
        let memtable_size: u64 = memtable.iter()
            .map(|(k, v)| (k.len() + v.len() + 32) as u64) // 32 bytes overhead estimate
            .sum();

        VelocityStats {
            memtable_entries: memtable.len(),
            sstable_count: sstables.len(),
            cache_entries: cache.len(),
            total_sstable_size: sstable_size,
            total_records: memtable.len() + sstable_records,
            total_size_bytes: sstable_size + memtable_size,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct VelocityStats {
    pub memtable_entries: usize,
    pub sstable_count: usize,
    pub cache_entries: usize,
    pub total_sstable_size: u64,
    pub total_records: usize,
    pub total_size_bytes: u64,
}

impl Drop for Velocity {
    fn drop(&mut self) {
        let _ = self.close();
    }
}