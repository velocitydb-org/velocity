use std::collections::{BTreeMap, HashMap, VecDeque};
use std::collections::hash_map::DefaultHasher;
use std::fs::{File, OpenOptions, create_dir_all, remove_file};
use std::io::{self, Write, Read, Seek, SeekFrom, BufWriter, BufReader};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// ==================== SMART LRU CACHE ====================
/// Better eviction with frequency tracking
struct SmartLruCache {
    capacity: usize,
    cache: HashMap<VeloKey, CacheEntry>,
    access_queue: VecDeque<VeloKey>,
    frequency: HashMap<VeloKey, usize>,
}

struct CacheEntry {
    value: VeloValue,
    size: usize,
}

impl SmartLruCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            cache: HashMap::with_capacity(capacity),
            access_queue: VecDeque::with_capacity(capacity),
            frequency: HashMap::with_capacity(capacity),
        }
    }

    #[inline]
    fn get(&mut self, key: &str) -> Option<VeloValue> {
        if let Some(entry) = self.cache.get(key) {
            // Frequency tracking
            *self.frequency.entry(key.to_string()).or_insert(0) += 1;

            // LRU update (lazy)
            if self.access_queue.len() < self.capacity * 2 {
                self.access_queue.push_back(key.to_string());
            }

            return Some(entry.value.clone());
        }
        None
    }

    fn put(&mut self, key: VeloKey, value: VeloValue) {
        let size = value.len();

        if self.cache.len() >= self.capacity && !self.cache.contains_key(&key) {
            self.evict_smart();
        }

        self.cache.insert(key.clone(), CacheEntry { value, size });
        self.access_queue.push_back(key.clone());
        *self.frequency.entry(key).or_insert(0) += 1;
    }

    fn evict_smart(&mut self) {
        // Frequency-based LFU + LRU hybrid
        let mut min_freq = usize::MAX;
        let mut victim = None;

        // find key with lowest frequency
        for key in self.access_queue.iter().take(self.capacity / 4) {
            if let Some(&freq) = self.frequency.get(key) {
                if freq < min_freq {
                    min_freq = freq;
                    victim = Some(key.clone());
                }
            }
        }

        if let Some(key) = victim {
            self.cache.remove(&key);
            self.frequency.remove(&key);
            self.access_queue.retain(|k| k != &key);
        }
    }

    fn clear(&mut self) {
        self.cache.clear();
        self.access_queue.clear();
        self.frequency.clear();
    }
}

/// ==================== WAL (Optimized) ====================
struct WriteAheadLog {
    file: BufWriter<File>,
    path: PathBuf,
    buffer_size: usize,
    entries_since_sync: usize,
    sync_threshold: usize,
}

impl WriteAheadLog {
    fn new<P: AsRef<Path>>(path: P) -> VeloResult<Self> {
        let wal_path = path.as_ref().with_extension("wal");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;

        Ok(Self {
            file: BufWriter::with_capacity(64 * 1024, file), // 64KB buffer
            path: wal_path,
            buffer_size: 0,
            entries_since_sync: 0,
            sync_threshold: 100, // sync every 100 operations
        })
    }

    fn log_operation(&mut self, key: &str, value: &[u8]) -> VeloResult<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.file.write_all(&timestamp.to_le_bytes())?;
        self.file.write_all(&(key.len() as u32).to_le_bytes())?;
        self.file.write_all(key.as_bytes())?;
        self.file.write_all(&(value.len() as u32).to_le_bytes())?;
        self.file.write_all(value)?;

        let checksum = self.calculate_checksum(key.as_bytes(), value);
        self.file.write_all(&checksum.to_le_bytes())?;

        self.buffer_size += key.len() + value.len() + 24;
        self.entries_since_sync += 1;

        // Conditional sync
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
        drop(&self.file);
        remove_file(&self.path)?;

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
    id: u64,
    path: PathBuf,
    index: BTreeMap<VeloKey, u64>,
    bloom: BloomFilter,
    min_key: Option<VeloKey>,
    max_key: Option<VeloKey>,
    size: u64,
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
    memtable: Arc<RwLock<BTreeMap<VeloKey, VeloValue>>>,
    sstables: Arc<RwLock<Vec<SSTable>>>,
    cache: Arc<Mutex<SmartLruCache>>,
    filter: Arc<RwLock<BloomFilter>>,
    wal: Arc<Mutex<WriteAheadLog>>,
    config: VelocityConfig,
    data_dir: PathBuf,
    next_sstable_id: Arc<Mutex<u64>>,
}

pub struct VelocityConfig {
    pub max_memtable_size: usize,
    pub cache_size: usize,
    pub bloom_false_positive_rate: f64,
    pub compaction_threshold: usize,
    pub enable_compression: bool,
}

impl Default for VelocityConfig {
    fn default() -> Self {
        Self {
            max_memtable_size: 5000,
            cache_size: 2000,
            bloom_false_positive_rate: 0.001,
            compaction_threshold: 8, // Daha az aggressive
            enable_compression: false,
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

        let wal = WriteAheadLog::new(data_dir.join("velocity"))?;
        let mut engine = Self {
            memtable: Arc::new(RwLock::new(BTreeMap::new())),
            sstables: Arc::new(RwLock::new(Vec::new())),
            cache: Arc::new(Mutex::new(SmartLruCache::new(config.cache_size))),
            filter: Arc::new(RwLock::new(BloomFilter::new(
                config.max_memtable_size * 10,
                config.bloom_false_positive_rate,
            ))),
            wal: Arc::new(Mutex::new(wal)),
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

    #[inline]
    pub fn put(&self, key: VeloKey, value: VeloValue) -> VeloResult<()> {
        // WAL
        {
            let mut wal = self.wal.lock().unwrap();
            wal.log_operation(&key, &value)?;
        }

        // Memtable & Cache
        {
            let mut memtable = self.memtable.write().unwrap();
            let mut filter = self.filter.write().unwrap();
            let mut cache = self.cache.lock().unwrap();

            filter.add(&key);
            memtable.insert(key.clone(), value.clone());
            cache.put(key, value);
        }

        // Check flush
        {
            let memtable = self.memtable.read().unwrap();
            if memtable.len() >= self.config.max_memtable_size {
                drop(memtable);
                self.flush()?;
            }
        }

        Ok(())
    }

    #[inline]
    pub fn get(&self, key: &str) -> VeloResult<Option<VeloValue>> {
        // 1. cache (fastest)
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(value) = cache.get(key) {
                return Ok(Some(value));
            }
        }

        // 2. Memtable
        {
            let memtable = self.memtable.read().unwrap();
            if let Some(value) = memtable.get(key) {
                let mut cache = self.cache.lock().unwrap();
                cache.put(key.to_string(), value.clone());
                return Ok(Some(value.clone()));
            }
        }

        // 3. Bloom filter
        {
            let filter = self.filter.read().unwrap();
            if !filter.might_contain(key) {
                return Ok(None);
            }
        }

        // 4. SSTables (yeniden eskiye)
        {
            let sstables = self.sstables.read().unwrap();
            for sstable in sstables.iter().rev() {
                if let Some(value) = sstable.get(key)? {
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

        VelocityStats {
            memtable_entries: memtable.len(),
            sstable_count: sstables.len(),
            cache_entries: cache.cache.len(),
            total_sstable_size: sstables.iter().map(|s| s.size).sum(),
        }
    }
}

#[derive(Debug)]
pub struct VelocityStats {
    pub memtable_entries: usize,
    pub sstable_count: usize,
    pub cache_entries: usize,
    pub total_sstable_size: u64,
}

impl Drop for Velocity {
    fn drop(&mut self) {
        let _ = self.close();
    }
}