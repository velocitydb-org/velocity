use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use velocity::addon::{
    BackgroundServiceAddonConfig, BackupAddonConfig, DatabaseAddonConfig,
};

pub fn default_bind_address() -> String {
    "127.0.0.1:2005".to_string()
}

pub fn default_max_connections() -> usize {
    1000
}

pub fn default_timeout() -> u64 {
    300
}

pub fn default_rate_limit() -> u32 {
    1000
}

pub fn default_log_level() -> String {
    "info".to_string()
}

pub fn default_bool_true() -> bool {
    true
}

pub fn default_metrics_interval() -> u64 {
    60
}

pub fn default_cache_hit_rate() -> f64 {
    0.85
}

pub fn default_audit_log_path() -> String {
    "./velocitydb_audit.log".to_string()
}

pub fn default_ban_duration() -> u64 {
    300
}

pub fn default_auth_failures() -> u32 {
    5
}

pub fn default_memtable() -> usize {
    10000
}

pub fn default_cache() -> usize {
    5000
}

pub fn default_bloom() -> f64 {
    0.001
}

pub fn default_compaction() -> usize {
    8
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfigSection {
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_timeout")]
    pub connection_timeout_seconds: u64,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_second: u32,
    #[serde(default)]
    pub enable_tls: bool,
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
pub struct LoggingSection {
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PerformanceSection {
    #[serde(default = "default_bool_true")]
    pub adaptive_cache: bool,
    #[serde(default = "default_bool_true")]
    pub enable_metrics: bool,
    #[serde(default = "default_metrics_interval")]
    pub metrics_interval: u64,
    #[serde(default = "default_cache_hit_rate")]
    pub target_cache_hit_rate: f64,
}

impl Default for PerformanceSection {
    fn default() -> Self {
        Self {
            adaptive_cache: true,
            enable_metrics: true,
            metrics_interval: default_metrics_interval(),
            target_cache_hit_rate: default_cache_hit_rate(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SecuritySection {
    #[serde(default = "default_audit_log_path")]
    pub audit_log_path: String,
    #[serde(default = "default_bool_true")]
    pub audit_logging: bool,
    #[serde(default = "default_ban_duration")]
    pub auth_ban_duration: u64,
    #[serde(default = "default_auth_failures")]
    pub max_auth_failures: u32,
}

impl Default for SecuritySection {
    fn default() -> Self {
        Self {
            audit_log_path: default_audit_log_path(),
            audit_logging: true,
            auth_ban_duration: default_ban_duration(),
            max_auth_failures: default_auth_failures(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DatabaseConfigSection {
    #[serde(default = "default_memtable")]
    pub max_memtable_size: usize,
    #[serde(default = "default_cache")]
    pub cache_size: usize,
    #[serde(default = "default_bloom")]
    pub bloom_false_positive_rate: f64,
    #[serde(default = "default_compaction")]
    pub compaction_threshold: usize,
    #[serde(default)]
    pub enable_compression: bool,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct AddonsSection {
    pub database: Option<DatabaseAddonConfig>,
    pub backup: Option<BackupAddonConfig>,
    #[serde(default, rename = "background-service", alias = "background_service")]
    pub background_service: Option<BackgroundServiceAddonConfig>,
}

impl Default for AddonsSection {
    fn default() -> Self {
        Self {
            database: None,
            backup: None,
            background_service: Some(BackgroundServiceAddonConfig::default()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigFile {
    pub server: ServerConfigSection,
    #[serde(default)]
    pub logging: LoggingSection,
    #[serde(default)]
    pub performance: PerformanceSection,
    #[serde(default)]
    pub security: SecuritySection,
    pub users: HashMap<String, String>,
    pub database: DatabaseConfigSection,
    #[serde(default)]
    pub addons: AddonsSection,
}

impl Default for ConfigFile {
    fn default() -> Self {
        let mut users = HashMap::new();
        users.insert(
            "admin".to_string(),
            "$argon2id$v=19$m=19456,t=2,p=1$GDWQpkPCnz9uM5W2SBpCmw$RNLHaiBA1s5wdbQSKJ28JzwD30wohA5KoB+W8MZOxic"
                .to_string(),
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
