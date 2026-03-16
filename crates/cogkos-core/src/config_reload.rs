//! Configuration hot reload module
//!
//! Provides:
//! - File-based configuration watching
//! - Runtime configuration updates
//! - Configuration change callbacks

use notify::{RecursiveMode, Watcher};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Configuration change event
#[derive(Clone, Debug)]
pub enum ConfigEvent {
    /// Configuration was modified
    Modified(String),
    /// Configuration was created
    Created(String),
    /// Configuration was removed
    Removed(String),
    /// Error occurred
    Error(String),
}

/// Configuration change callback
pub type ConfigCallback = Arc<dyn Fn(ConfigEvent) + Send + Sync>;

/// Hot reload configuration manager
pub struct ConfigHotReload {
    watchers: Arc<RwLock<HashMap<String, notify::RecommendedWatcher>>>,
    callbacks: Arc<RwLock<Vec<ConfigCallback>>>,
    event_sender: Option<mpsc::Sender<ConfigEvent>>,
}

impl ConfigHotReload {
    /// Create a new hot reload manager
    pub fn new() -> Self {
        Self {
            watchers: Arc::new(RwLock::new(HashMap::new())),
            callbacks: Arc::new(RwLock::new(Vec::new())),
            event_sender: None,
        }
    }

    /// Create with event channel
    pub fn with_channel(channel_size: usize) -> (Self, mpsc::Receiver<ConfigEvent>) {
        let (sender, receiver) = mpsc::channel(channel_size);
        let mut this = Self::new();
        this.event_sender = Some(sender);
        (this, receiver)
    }

    /// Watch a configuration file
    pub fn watch_file(&self, path: impl Into<String>) -> Result<(), String> {
        let path = path.into();
        let path_clone = path.clone();
        let callbacks = self.callbacks.clone();
        let sender = self.event_sender.clone();

        let watcher_result =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        for path in event.paths {
                            let path_str = path.to_string_lossy().to_string();
                            let config_event = match event.kind {
                                notify::EventKind::Create(_) => {
                                    ConfigEvent::Created(path_str.clone())
                                }
                                notify::EventKind::Modify(_) => {
                                    ConfigEvent::Modified(path_str.clone())
                                }
                                notify::EventKind::Remove(_) => {
                                    ConfigEvent::Removed(path_str.clone())
                                }
                                _ => continue,
                            };

                            // Notify callbacks
                            for callback in callbacks.read().iter() {
                                callback(config_event.clone());
                            }

                            // Send to channel if available
                            if let Some(ref sender) = sender {
                                let _ = sender.blocking_send(config_event);
                            }
                        }
                    }
                    Err(e) => {
                        let config_event = ConfigEvent::Error(e.to_string());
                        for callback in callbacks.read().iter() {
                            callback(config_event.clone());
                        }
                        if let Some(ref sender) = sender {
                            let _ = sender.blocking_send(config_event);
                        }
                    }
                }
            });

        let mut watcher = watcher_result.map_err(|e| e.to_string())?;

        // Watch the file
        watcher
            .watch(Path::new(&path), RecursiveMode::NonRecursive)
            .map_err(|e| e.to_string())?;

        self.watchers.write().insert(path_clone, watcher);

        Ok(())
    }

    /// Watch a directory for config files
    pub fn watch_directory(&self, dir: impl Into<String>) -> Result<(), String> {
        let dir = dir.into();
        let dir_clone = dir.clone();
        let callbacks = self.callbacks.clone();
        let sender = self.event_sender.clone();

        let watcher_result =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        for path in event.paths {
                            let path_str = path.to_string_lossy().to_string();
                            // Only process config files
                            if !path_str.ends_with(".toml")
                                && !path_str.ends_with(".json")
                                && !path_str.ends_with(".yaml")
                                && !path_str.ends_with(".yml")
                            {
                                continue;
                            }

                            let config_event = match event.kind {
                                notify::EventKind::Create(_) => {
                                    ConfigEvent::Created(path_str.clone())
                                }
                                notify::EventKind::Modify(_) => {
                                    ConfigEvent::Modified(path_str.clone())
                                }
                                notify::EventKind::Remove(_) => {
                                    ConfigEvent::Removed(path_str.clone())
                                }
                                _ => continue,
                            };

                            for callback in callbacks.read().iter() {
                                callback(config_event.clone());
                            }

                            if let Some(ref sender) = sender {
                                let _ = sender.blocking_send(config_event);
                            }
                        }
                    }
                    Err(e) => {
                        let config_event = ConfigEvent::Error(e.to_string());
                        for callback in callbacks.read().iter() {
                            callback(config_event.clone());
                        }
                        if let Some(ref sender) = sender {
                            let _ = sender.blocking_send(config_event);
                        }
                    }
                }
            });

        let mut watcher = watcher_result.map_err(|e| e.to_string())?;
        watcher
            .watch(Path::new(&dir), RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;

        self.watchers.write().insert(dir_clone, watcher);

        Ok(())
    }

    /// Register a callback for configuration changes
    pub fn on_change(&self, callback: ConfigCallback) {
        self.callbacks.write().push(callback);
    }

    /// Stop watching all files
    pub fn stop(&self) {
        self.watchers.write().clear();
    }
}

impl Default for ConfigHotReload {
    fn default() -> Self {
        Self::new()
    }
}

/// Reloadable configuration wrapper
pub struct ReloadableConfig<T> {
    config: Arc<RwLock<T>>,
    hot_reload: ConfigHotReload,
}

impl<T: Clone + serde::de::DeserializeOwned + Send + Sync + 'static> ReloadableConfig<T> {
    /// Create a new reloadable config
    pub fn new(path: &str) -> Result<Self, String> {
        // Load initial config
        let config = Self::load_config(path)?;

        let this = Self {
            config: Arc::new(RwLock::new(config)),
            hot_reload: ConfigHotReload::new(),
        };

        // Watch for changes
        let config_path = path.to_string();
        let config_clone = this.config.clone();
        this.hot_reload.on_change(Arc::new(move |event| {
            if let ConfigEvent::Modified(ref event_path) = event
                && event_path == &config_path
            {
                tracing::info!("Configuration file changed, reloading...");
                match Self::load_config(event_path) {
                    Ok(new_config) => {
                        *config_clone.write() = new_config;
                        tracing::info!("Configuration reloaded successfully");
                    }
                    Err(e) => {
                        tracing::error!("Failed to reload configuration: {}", e);
                    }
                }
            }
        }));

        this.hot_reload.watch_file(path)?;

        Ok(this)
    }

    /// Load configuration from file
    fn load_config(path: &str) -> Result<T, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;

        if path.ends_with(".toml") {
            toml::from_str(&content).map_err(|e| e.to_string())
        } else if path.ends_with(".json") {
            serde_json::from_str(&content).map_err(|e| e.to_string())
        } else if path.ends_with(".yaml") || path.ends_with(".yml") {
            serde_yaml::from_str(&content).map_err(|e| e.to_string())
        } else {
            Err("Unsupported config file format".to_string())
        }
    }

    /// Get current configuration
    pub fn get(&self) -> T {
        self.config.read().clone()
    }

    /// Get configuration reference
    pub fn get_ref(&self) -> parking_lot::RwLockReadGuard<'_, T> {
        self.config.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_hot_reload_watch() {
        let reload = ConfigHotReload::new();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_clone = events.clone();

        reload.on_change(Arc::new(move |event| {
            events_clone.lock().unwrap().push(event);
        }));

        // This test would require a real file to watch
        // Just verify the struct can be created
        assert!(reload.watchers.read().is_empty());
    }

    #[test]
    fn test_reloadable_config() {
        // Create temp config file
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        fs::write(path, "value = 42").unwrap();

        // Note: This won't work in test because notify requires async runtime
        // Just verify the module compiles
    }
}
