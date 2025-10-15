use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize, Debug)]
pub struct RepoContent {
    pub name: String,
    #[serde(rename = "type")]
    pub content_type: String,
    pub download_url: Option<String>,
    pub path: String,
}

#[derive(Deserialize, Debug)]
pub struct RateLimitResponse {
    pub resources: RateLimitResources,
}

#[derive(Deserialize, Debug)]
pub struct RateLimitResources {
    pub core: RateLimit,
}

#[derive(Deserialize, Debug)]
pub struct RateLimit {
    pub limit: u32,
    pub remaining: u32,
    pub reset: u64,
}

#[derive(Debug, Default)]
pub struct TemplateIndex {
    pub templates: BTreeMap<String, String>,
}

impl TemplateIndex {
    pub fn new() -> Self {
        Self {
            templates: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, name: String, path: String) {
        self.templates.insert(name, path);
    }

    pub fn list(&self) -> Vec<String> {
        self.templates.keys().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<&String> {
        self.templates.get(name)
    }

    pub fn write(&self, cache_dir: &PathBuf) -> Result<()> {
        let index_path = cache_dir.join("index.json");
        let data = serde_json::to_vec_pretty(&self.templates)?;
        fs::write(index_path, data)?;
        Ok(())
    }

    pub fn read(cache_dir: &PathBuf) -> Result<Self> {
        let index_path = cache_dir.join("index.json");
        if !index_path.exists() {
            anyhow::bail!("index not found. Please run `lignore update` first");
        }
        let data = fs::read(index_path)?;
        let templates: BTreeMap<String, String> = serde_json::from_slice(&data)?;
        Ok(TemplateIndex { templates })
    }
}
