use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// Trait for persisting authentication tokens keyed by core_id.
///
/// Implement this trait to provide custom storage backends. The SDK
/// ships with `FileTokenStore` (JSON file) and `MemoryTokenStore` (in-memory).
pub trait TokenStore: Send + Sync + 'static {
    fn load_token(&self, core_id: &str) -> Option<String>;
    fn save_token(&self, core_id: &str, token: &str) -> Result<(), String>;
}

/// In-memory token store for testing.
#[derive(Debug, Default)]
pub struct MemoryTokenStore {
    tokens: Mutex<HashMap<String, String>>,
}

impl MemoryTokenStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TokenStore for MemoryTokenStore {
    fn load_token(&self, core_id: &str) -> Option<String> {
        self.tokens.lock().unwrap().get(core_id).cloned()
    }

    fn save_token(&self, core_id: &str, token: &str) -> Result<(), String> {
        self.tokens
            .lock()
            .unwrap()
            .insert(core_id.to_string(), token.to_string());
        Ok(())
    }
}

/// File-based token store that persists tokens as JSON.
#[derive(Debug)]
pub struct FileTokenStore {
    path: PathBuf,
}

impl FileTokenStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn read_all(&self) -> HashMap<String, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    fn write_all(&self, tokens: &HashMap<String, String>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create token directory: {}", e))?;
        }
        let content =
            serde_json::to_string_pretty(tokens).map_err(|e| format!("JSON error: {}", e))?;
        std::fs::write(&self.path, content)
            .map_err(|e| format!("failed to write token file: {}", e))
    }
}

impl TokenStore for FileTokenStore {
    fn load_token(&self, core_id: &str) -> Option<String> {
        self.read_all().get(core_id).cloned()
    }

    fn save_token(&self, core_id: &str, token: &str) -> Result<(), String> {
        let mut tokens = self.read_all();
        tokens.insert(core_id.to_string(), token.to_string());
        self.write_all(&tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_store_roundtrip() {
        let store = MemoryTokenStore::new();
        assert!(store.load_token("core-1").is_none());

        store.save_token("core-1", "token-abc").unwrap();
        assert_eq!(store.load_token("core-1").unwrap(), "token-abc");

        store.save_token("core-1", "token-xyz").unwrap();
        assert_eq!(store.load_token("core-1").unwrap(), "token-xyz");
    }

    #[test]
    fn test_file_store_roundtrip() {
        let dir = std::env::temp_dir().join("roon-api-test-tokens");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("tokens.json");

        let store = FileTokenStore::new(&path);
        assert!(store.load_token("core-1").is_none());

        store.save_token("core-1", "token-abc").unwrap();
        assert_eq!(store.load_token("core-1").unwrap(), "token-abc");

        // Verify persistence across instances
        let store2 = FileTokenStore::new(&path);
        assert_eq!(store2.load_token("core-1").unwrap(), "token-abc");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
