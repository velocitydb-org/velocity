use crate::{VeloError, VeloResult, Velocity, VelocityConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseAddonConfig {
    pub enabled: bool,
    pub default_path: PathBuf,
    #[serde(default)]
    pub databases: HashMap<String, PathBuf>,
}

impl Default for DatabaseAddonConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_path: PathBuf::from("./externals_dbs"),
            databases: HashMap::new(),
        }
    }
}


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackupAddonConfig {
    pub enabled: bool,
    pub backup_path: PathBuf,
    pub interval_minutes: u64,
    pub backup_all: bool,
    #[serde(default)]
    pub target_databases: Vec<String>,
}

impl Default for BackupAddonConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backup_path: PathBuf::from("./backups"),
            interval_minutes: 60,
            backup_all: true,
            target_databases: Vec::new(),
        }
    }
}


pub struct DatabaseManager {
    default_db: Arc<Velocity>,
    databases: RwLock<HashMap<String, Arc<Velocity>>>,
    pub db_config: RwLock<DatabaseAddonConfig>,
    backup_config: RwLock<BackupAddonConfig>,
    config_path: PathBuf,
}

impl DatabaseManager {
    pub fn new(default_db: Velocity, config_path: PathBuf) -> Self {
        let manager = Self {
            default_db: Arc::new(default_db),
            databases: RwLock::new(HashMap::new()),
            db_config: RwLock::new(DatabaseAddonConfig::default()),
            backup_config: RwLock::new(BackupAddonConfig::default()),
            config_path,
        };


        let _ = manager.reload_config();

        manager
    }

    pub fn reload_config(&self) -> VeloResult<()> {
        if !self.config_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.config_path)?;
        let toml_value: toml::Value = toml::from_str(&content)
            .map_err(|e| VeloError::InvalidOperation(format!("Config parse error: {}", e)))?;

        let mut db_config = DatabaseAddonConfig::default();
        let mut backup_config = BackupAddonConfig::default();

        if let Some(addons) = toml_value.get("addons") {
            if let Some(db_addon) = addons.get("database") {
                db_config = db_addon.clone().try_into().map_err(|e| {
                    VeloError::InvalidOperation(format!("Database addon config error: {}", e))
                })?;
            }
            if let Some(backup_addon) = addons.get("backup") {
                backup_config = backup_addon.clone().try_into().map_err(|e| {
                    VeloError::InvalidOperation(format!("Backup addon config error: {}", e))
                })?;
            }
        }


        if db_config.enabled {
            let mut dbs = self.databases.write().unwrap();

            if !db_config.default_path.exists() {
                fs::create_dir_all(&db_config.default_path)?;
            }

            for (name, path) in &db_config.databases {
                if !dbs.contains_key(name) {
                    match Velocity::open(path) {
                        Ok(db) => {
                            dbs.insert(name.clone(), Arc::new(db));
                            log::info!("Loaded database '{}' from {:?}", name, path);
                        }
                        Err(e) => log::error!("Failed to load database '{}': {}", name, e),
                    }
                }
            }
        }

        *self.db_config.write().unwrap() = db_config;
        *self.backup_config.write().unwrap() = backup_config;

        Ok(())
    }

    pub fn save_config(&self) -> VeloResult<()> {
        let content = fs::read_to_string(&self.config_path).unwrap_or_default();
        let mut toml_value: toml::Value =
            toml::from_str(&content).unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()));

        let db_config = self.db_config.read().unwrap();
        let backup_config = self.backup_config.read().unwrap();

        let db_addon_val = toml::Value::try_from(&*db_config)
            .map_err(|e| VeloError::InvalidOperation(format!("DB Serialization error: {}", e)))?;
        let backup_addon_val = toml::Value::try_from(&*backup_config).map_err(|e| {
            VeloError::InvalidOperation(format!("Backup Serialization error: {}", e))
        })?;

        if toml_value.get("addons").is_none() {
            toml_value.as_table_mut().unwrap().insert(
                "addons".to_string(),
                toml::Value::Table(toml::map::Map::new()),
            );
        }

        if let Some(addons) = toml_value.get_mut("addons") {
            if let Some(addons_table) = addons.as_table_mut() {
                addons_table.insert("database".to_string(), db_addon_val);
                addons_table.insert("backup".to_string(), backup_addon_val);
            }
        }

        let new_content = toml::to_string_pretty(&toml_value)
            .map_err(|e| VeloError::InvalidOperation(format!("TOML serialization error: {}", e)))?;

        fs::write(&self.config_path, new_content)?;

        Ok(())
    }

    pub fn create_database(&self, name: &str, path: Option<&str>) -> VeloResult<()> {

        {
            let dbs = self.databases.read().unwrap();
            if dbs.contains_key(name) || name == "default" {
                return Err(VeloError::InvalidOperation(format!(
                    "Database '{}' already exists",
                    name
                )));
            }
        }

        let mut config = self.db_config.write().unwrap();


        let db_path = if let Some(p) = path {
            PathBuf::from(p)
        } else {
            config.default_path.join(name)
        };


        if !db_path.exists() {
            fs::create_dir_all(&db_path)?;
        }


        let db = Velocity::open(&db_path)?;


        let mut dbs = self.databases.write().unwrap();
        dbs.insert(name.to_string(), Arc::new(db));


        config.databases.insert(name.to_string(), db_path.clone());


        drop(config);
        drop(dbs);


        self.save_config()?;

        log::info!("Created new database '{}' at {:?}", name, db_path);
        Ok(())
    }

    pub fn drop_database(&self, name: &str) -> VeloResult<()> {
        if name == "default" {
            return Err(VeloError::InvalidOperation(
                "Cannot drop the default database".to_string(),
            ));
        }

        let mut config = self.db_config.write().unwrap();
        let mut dbs = self.databases.write().unwrap();


        if !dbs.contains_key(name) {
            return Err(VeloError::KeyNotFound(format!(
                "Database '{}' not found",
                name
            )));
        }


        let db_path = config.databases.get(name).cloned();


        dbs.remove(name);


        config.databases.remove(name);


        drop(config);
        drop(dbs);
        self.save_config()?;


        if let Some(path) = db_path {
            if path.exists() {
                fs::remove_dir_all(path)?;
            }
        }

        log::info!("Dropped database '{}'", name);
        Ok(())
    }

    pub fn get_database(&self, name: &str) -> Option<Arc<Velocity>> {
        if name == "default" {
            return Some(self.default_db.clone());
        }

        let db_configs = self.db_config.read().unwrap();
        if !db_configs.enabled {
            return None;
        }

        let dbs = self.databases.read().unwrap();
        dbs.get(name).cloned()
    }

    pub fn list_databases(&self) -> Vec<String> {
        let db_configs = self.db_config.read().unwrap();
        if !db_configs.enabled {
            return vec!["default".to_string()];
        }

        let dbs = self.databases.read().unwrap();
        let mut list: Vec<String> = dbs.keys().cloned().collect();
        list.push("default".to_string());
        list
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AddonKind {
    Database,
    Backup,
}

impl AddonKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AddonKind::Database => "database",
            AddonKind::Backup => "backup",
        }
    }
}

impl DatabaseManager {
    pub fn toggle_addon(&self, kind: AddonKind, enabled: bool) -> VeloResult<()> {
        match kind {
            AddonKind::Database => {
                let mut config = self.db_config.write().unwrap();
                config.enabled = enabled;
            }
            AddonKind::Backup => {
                let mut config = self.backup_config.write().unwrap();
                config.enabled = enabled;
            }
        }
        self.save_config()
    }

    pub fn list_addons(&self) -> Vec<(String, bool)> {
        let db_enabled = self.db_config.read().unwrap().enabled;
        let backup_enabled = self.backup_config.read().unwrap().enabled;

        vec![
            ("database".to_string(), db_enabled),
            ("backup".to_string(), backup_enabled),
        ]
    }

    pub fn backup_all_databases(&self) -> VeloResult<Vec<String>> {
        let config = self.backup_config.read().unwrap();
        if !config.enabled {
            return Err(VeloError::InvalidOperation(
                "Backup addon is disabled".to_string(),
            ));
        }

        if !config.backup_path.exists() {
            fs::create_dir_all(&config.backup_path)?;
        }

        let mut successful_backups = Vec::new();
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();

        let databases_to_backup = if config.backup_all {
            self.list_databases()
        } else {
            config.target_databases.clone()
        };

        for db_name in databases_to_backup {
            let backup_dir = config.backup_path.join(&db_name).join(&timestamp);
            fs::create_dir_all(&backup_dir)?;

            let db_path = if db_name == "default" {
                continue;
            } else {
                let db_configs = self.db_config.read().unwrap();
                if let Some(path) = db_configs.databases.get(&db_name) {
                    path.clone()
                } else {
                    continue;
                }
            };

            if db_path.exists() {
                self.copy_dir(&db_path, &backup_dir)?;
                successful_backups.push(db_name);
            }
        }

        Ok(successful_backups)
    }

    fn copy_dir(&self, src: &Path, dst: &Path) -> VeloResult<()> {
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                fs::copy(&path, dst.join(entry.file_name()))?;
            }
        }
        Ok(())
    }

    pub fn stats(&self) -> crate::VelocityStats {
        let default_stats = self.default_db.stats();
        let mut agg_stats = default_stats;

        let db_configs = self.db_config.read().unwrap();
        if db_configs.enabled {
            let dbs = self.databases.read().unwrap();
            for db in dbs.values() {
                let s = db.stats();
                agg_stats.memtable_entries += s.memtable_entries;
                agg_stats.sstable_count += s.sstable_count;
                agg_stats.cache_entries += s.cache_entries;
                agg_stats.total_sstable_size += s.total_sstable_size;
            }
        }

        agg_stats
    }
}
