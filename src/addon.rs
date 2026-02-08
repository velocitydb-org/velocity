use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::fs;
use serde::{Deserialize, Serialize};
use crate::{Velocity, VelocityConfig, VeloResult, VeloError};

/// Database Addon Configuration
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

/// Database Manager
pub struct DatabaseManager {
    default_db: Arc<Velocity>,
    databases: RwLock<HashMap<String, Arc<Velocity>>>,
    config: RwLock<DatabaseAddonConfig>,
    config_path: PathBuf,
}

impl DatabaseManager {
    pub fn new(default_db: Velocity, config_path: PathBuf) -> Self {
        let manager = Self {
            default_db: Arc::new(default_db),
            databases: RwLock::new(HashMap::new()),
            config: RwLock::new(DatabaseAddonConfig::default()),
            config_path,
        };
        
        // Initial load
        let _ = manager.reload_config();
        
        manager
    }

    pub fn reload_config(&self) -> VeloResult<()> {
        if !self.config_path.exists() {
            // If config doesn't exist, just use defaults
            return Ok(());
        }

        let content = fs::read_to_string(&self.config_path)?;
        let toml_value: toml::Value = toml::from_str(&content)
            .map_err(|e| VeloError::InvalidOperation(format!("Config parse error: {}", e)))?;

        let mut config = DatabaseAddonConfig::default();

        if let Some(addons) = toml_value.get("addons") {
            if let Some(db_addon) = addons.get("database") {
                config = db_addon.clone().try_into()
                    .map_err(|e| VeloError::InvalidOperation(format!("Addon config error: {}", e)))?;
            }
        }
        
        // Apply config
        if config.enabled {
            let mut dbs = self.databases.write().unwrap();
            
            // Create default storage if needed
            if !config.default_path.exists() {
                fs::create_dir_all(&config.default_path)?;
            }
            
            for (name, path) in &config.databases {
                if !dbs.contains_key(name) {
                    if path.exists() {
                        match Velocity::open(path) {
                            Ok(db) => {
                                dbs.insert(name.clone(), Arc::new(db));
                                log::info!("Loaded external database '{}' from {:?}", name, path);
                            }
                            Err(e) => {
                                log::error!("Failed to load database '{}': {}", name, e);
                            }
                        }
                    } else {
                        // Attempt to create if it is configured but missing?
                        // Or maybe just log warning.
                        // Let's create it if missing, similar to 'open' behavior.
                        match Velocity::open(path) {
                             Ok(db) => {
                                dbs.insert(name.clone(), Arc::new(db));
                                log::info!("Initialized configured database '{}' at {:?}", name, path);
                            }
                            Err(e) => {
                                log::error!("Failed to initialize database '{}': {}", name, e);
                            }
                        }
                    }
                }
            }
        }
        
        *self.config.write().unwrap() = config;

        Ok(())
    }

    pub fn save_config(&self) -> VeloResult<()> {
        let content = fs::read_to_string(&self.config_path).unwrap_or_default();
        let mut toml_value: toml::Value = toml::from_str(&content).unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()));

        let config = self.config.read().unwrap();
        
        // Serialize our config
        let addon_config_value = toml::Value::try_from(&*config)
             .map_err(|e| VeloError::InvalidOperation(format!("Serialization error: {}", e)))?;

        // Ensure [addons] table exists
        if toml_value.get("addons").is_none() {
             toml_value.as_table_mut().unwrap().insert("addons".to_string(), toml::Value::Table(toml::map::Map::new()));
        }

        // Update [addons.database]
        if let Some(addons) = toml_value.get_mut("addons") {
            if let Some(addons_table) = addons.as_table_mut() {
                addons_table.insert("database".to_string(), addon_config_value);
            }
        }

        let new_content = toml::to_string_pretty(&toml_value)
            .map_err(|e| VeloError::InvalidOperation(format!("TOML serialization error: {}", e)))?;
            
        fs::write(&self.config_path, new_content)?;
        
        Ok(())
    }

    pub fn create_database(&self, name: &str, path: Option<&str>) -> VeloResult<()> {
        // First check
        {
            let dbs = self.databases.read().unwrap();
            if dbs.contains_key(name) || name == "default" {
                return Err(VeloError::InvalidOperation(format!("Database '{}' already exists", name)));
            }
        }
        
        let mut config = self.config.write().unwrap();
        
        // Determine path
        let db_path = if let Some(p) = path {
            PathBuf::from(p)
        } else {
            config.default_path.join(name)
        };

        // Create directory if it doesn't exist
        if !db_path.exists() {
            fs::create_dir_all(&db_path)?;
        }

        // Initialize VelocityDB at path
        let db = Velocity::open(&db_path)?;
        
        // Register in memory
        let mut dbs = self.databases.write().unwrap();
        dbs.insert(name.to_string(), Arc::new(db));
        
        // Update config
        config.databases.insert(name.to_string(), db_path.clone());
        
        // Release lock
        drop(config); 
        drop(dbs);

        // Save to disk
        self.save_config()?;

        log::info!("Created new database '{}' at {:?}", name, db_path);
        Ok(())
    }

    pub fn drop_database(&self, name: &str) -> VeloResult<()> {
        if name == "default" {
            return Err(VeloError::InvalidOperation("Cannot drop the default database".to_string()));
        }

        let mut config = self.config.write().unwrap();
        let mut dbs = self.databases.write().unwrap();

        // Check exists
        if !dbs.contains_key(name) {
            return Err(VeloError::KeyNotFound(format!("Database '{}' not found", name)));
        }

        // Get path before removing from config
        let db_path = config.databases.get(name).cloned();

        // 1. Remove from memory
        dbs.remove(name);

        // 2. Remove from config
        config.databases.remove(name);

        // 3. Save config
        drop(config); 
        drop(dbs);
        self.save_config()?;

        // 4. Remove from disk
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
        
        let dbs = self.databases.read().unwrap();
        dbs.get(name).cloned()
    }
    
    pub fn list_databases(&self) -> Vec<String> {
        let dbs = self.databases.read().unwrap();
        let mut list: Vec<String> = dbs.keys().cloned().collect();
        list.push("default".to_string());
        list
    }

    pub fn stats(&self) -> crate::VelocityStats {
        let default_stats = self.default_db.stats();
        let mut agg_stats = default_stats;
        
        let dbs = self.databases.read().unwrap();
        for db in dbs.values() {
            let s = db.stats();
            agg_stats.memtable_entries += s.memtable_entries;
            agg_stats.sstable_count += s.sstable_count;
            agg_stats.cache_entries += s.cache_entries;
            agg_stats.total_sstable_size += s.total_sstable_size;
        }
        
        agg_stats
    }
}
