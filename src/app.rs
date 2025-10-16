use crate::ui::theme::get_theme;
use anyhow::{Context, Result};
use crossterm::{
    QueueableCommand,
    style::{Print, ResetColor, SetForegroundColor},
};
use futures::stream::{self, StreamExt};
use reqwest::Client;
use std::fs;
use std::future::Future;
use std::io::{self, Write};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{
    build_options_list, build_previous_selection, load_or_default_config, update_and_save_config,
    validate_config,
};
use crate::gitignore::{ensure_output_directory, generate_gitignore_content};
use crate::template::{RateLimit, RepoContent, TemplateIndex};
use crate::ui::display::print_success_message;
use crate::ui::{calculate_column_layout, print_columnar_list, select_templates};
use crate::validation::{validate_output_path, validate_template_key};

// Security limits
pub const MAX_DOWNLOAD_SIZE: u64 = 10 * 1024 * 1024; // 10MB

pub const GITIGNORE_REPO_API: &str = "https://api.github.com/repos/github/gitignore";

pub struct App {
    client: Client,
    cache_dir: PathBuf,
}

impl App {
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        let client = Client::builder()
            .user_agent("lightignore/0.1")
            .build()
            .context("building HTTP client")?;
        Ok(Self { client, cache_dir })
    }

    fn ensure_cache_dir(&self) -> Result<()> {
        if !self.cache_dir.exists() {
            fs::create_dir_all(&self.cache_dir).with_context(|| {
                format!("creating cache directory at {}", self.cache_dir.display())
            })?;
        }
        Ok(())
    }

    async fn fetch_repo_tree(&self, path: &str) -> Result<Vec<RepoContent>> {
        let url = format!("{}/contents/{}", GITIGNORE_REPO_API, path);
        let res = self
            .client
            .get(url)
            .send()
            .await
            .context("fetching repository contents")?;
        if !res.status().is_success() {
            if res.status().as_u16() == 403 {
                self.display_rate_limit_info().await;
            }
            anyhow::bail!("GitHub API returned status {}", res.status());
        }
        let contents = res
            .json::<Vec<RepoContent>>()
            .await
            .context("parsing GitHub contents response")?;
        Ok(contents)
    }

    async fn fetch_rate_limit_info(&self) -> Result<RateLimit> {
        use crate::template::RateLimitResponse;

        let url = "https://api.github.com/rate_limit";
        let res = self
            .client
            .get(url)
            .send()
            .await
            .context("fetching rate limit info")?;
        let data = res
            .json::<RateLimitResponse>()
            .await
            .context("parsing rate limit response")?;
        Ok(data.resources.core)
    }

    async fn display_rate_limit_info(&self) {
        if let Ok(rate_limit) = self.fetch_rate_limit_info().await {
            let mut stdout = io::stdout();
            let theme = get_theme();
            let _ = stdout.queue(SetForegroundColor(theme.header_title));
            let _ = stdout.queue(Print("\nRate Limit Information:\n"));
            let _ = stdout.queue(ResetColor);

            let _ = stdout.queue(SetForegroundColor(theme.accent));
            let _ = stdout.queue(Print(format!("  Limit:     {}\n", rate_limit.limit)));
            let _ = stdout.queue(Print(format!("  Remaining: {}\n", rate_limit.remaining)));

            // Convert reset timestamp to human-readable format
            let reset_time = rate_limit.reset;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let wait_time = if reset_time > now {
                reset_time - now
            } else {
                0
            };

            let minutes = wait_time / 60;
            let seconds = wait_time % 60;

            let _ = stdout.queue(Print(format!(
                "  Reset:     {} (in {}m {}s)\n",
                reset_time, minutes, seconds
            )));
            let _ = stdout.queue(ResetColor);
            let _ = stdout.flush();
        }
    }

    pub async fn update_cache(&self) -> Result<TemplateIndex> {
        self.ensure_cache_dir()?;

        // Phase 1: Collect all template URLs
        println!("Scanning gitignore repository...");
        let templates = self.collect_templates_recursive("").await?;

        println!("Found {} templates. Downloading...", templates.len());

        // Phase 2: Download templates in parallel with progress tracking
        let counter = Arc::new(AtomicUsize::new(0));
        let total = templates.len();

        let results = stream::iter(templates)
            .map(|(key, name, download_url)| {
                let counter = Arc::clone(&counter);
                async move {
                    let result = self.download_template(&key, &download_url).await;
                    let current = counter.fetch_add(1, Ordering::SeqCst) + 1;

                    // Print progress every 10 templates or on the last one
                    if current % 10 == 0 || current == total {
                        print!("\rDownloaded {}/{} templates", current, total);
                        let _ = io::stdout().flush();
                    }

                    result.map(|path| (name, path))
                }
            })
            .buffer_unordered(20) // Download 20 templates concurrently
            .collect::<Vec<_>>()
            .await;

        println!(); // New line after progress

        // Build index from results
        let mut index = TemplateIndex::new();
        for result in results {
            match result {
                Ok((name, path)) => {
                    index.insert(name, path.to_string_lossy().to_string());
                }
                Err(e) => {
                    eprintln!("Warning: Failed to download template: {}", e);
                }
            }
        }

        index.write(&self.cache_dir)?;
        Ok(index)
    }

    // Collect all template information without downloading
    fn collect_templates_recursive<'a>(
        &'a self,
        path: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<(String, String, String)>>> + 'a>> {
        Box::pin(async move {
            let contents = self.fetch_repo_tree(path).await?;
            let mut templates = Vec::new();

            for entry in contents {
                if entry.content_type == "file" && entry.name.ends_with(".gitignore") {
                    if let Some(download_url) = entry.download_url {
                        let name = entry.name.trim_end_matches(".gitignore").to_string();
                        // Use the full path as the cache key to avoid conflicts
                        let cache_key = if path.is_empty() {
                            name.clone()
                        } else {
                            format!("{}/{}", path, name)
                        };
                        templates.push((cache_key, name, download_url));
                    }
                } else if entry.content_type == "dir" {
                    let mut sub_templates = self.collect_templates_recursive(&entry.path).await?;
                    templates.append(&mut sub_templates);
                }
            }

            Ok(templates)
        })
    }

    async fn download_template(&self, key: &str, url: &str) -> Result<PathBuf> {
        // Validate key to prevent path traversal
        validate_template_key(key)?;

        if !url.starts_with("https://") {
            anyhow::bail!("Download URL must use HTTPS: {}", url);
        }

        let sanitized_key = key.replace('/', "_");
        let file_path = self.cache_dir.join(format!("{}.gitignore", sanitized_key));

        let response = self
            .client
            .get(url)
            .send()
            .await
            .with_context(|| format!("downloading template {}", key))?;

        if !response.status().is_success() {
            if response.status().as_u16() == 403 {
                self.display_rate_limit_info().await;
            }
            anyhow::bail!(
                "failed to download template {}: status {}",
                key,
                response.status()
            );
        }

        if let Some(content_length) = response.content_length() {
            if content_length > MAX_DOWNLOAD_SIZE {
                anyhow::bail!(
                    "Template {} is too large: {} bytes (max: {} bytes)",
                    key,
                    content_length,
                    MAX_DOWNLOAD_SIZE
                );
            }
        }

        let content = response.text().await?;

        // Double-check size after download
        if content.len() > MAX_DOWNLOAD_SIZE as usize {
            anyhow::bail!(
                "Template {} exceeds size limit: {} bytes (max: {} bytes)",
                key,
                content.len(),
                MAX_DOWNLOAD_SIZE
            );
        }

        fs::write(&file_path, content)
            .with_context(|| format!("writing template {} to cache", key))?;

        Ok(file_path)
    }

    pub fn read_index(&self) -> Result<TemplateIndex> {
        TemplateIndex::read(&self.cache_dir)
    }

    /// Read index from cache, or automatically update cache if it doesn't exist
    pub fn read_index_or_update(&self, rt: &tokio::runtime::Runtime) -> Result<TemplateIndex> {
        match self.read_index() {
            Ok(index) => Ok(index),
            Err(_) => {
                println!("No cache found. Downloading templates for the first time...");
                println!(
                    "(This is a one-time setup and will be much faster with parallel downloads)\n"
                );
                rt.block_on(self.update_cache())
            }
        }
    }

    pub fn list_templates(&self, index: &TemplateIndex) -> Result<()> {
        let items = index.list();
        if items.is_empty() {
            println!("No templates found. Run `lignore update` first.");
            return Ok(());
        }

        let layout = calculate_column_layout(&items)?;
        print_columnar_list(&items, &layout)
    }

    pub fn generate_interactive(&self, index: &TemplateIndex, output: PathBuf) -> Result<()> {
        // Validate output path
        validate_output_path(&output)
            .with_context(|| format!("validating output path: {}", output.display()))?;

        let options = index.list();
        if options.is_empty() {
            println!("No templates available. Run `lignore update` first.");
            return Ok(());
        }

        // Load and validate config
        let config_path = PathBuf::from("lignore.json");
        let mut config = load_or_default_config(&config_path);
        validate_config(&options, &config)?;

        // Build options and selection lists
        let all_options = build_options_list(&options, &config);
        let previous_selection = build_previous_selection(&options, &config);

        // Interactive selection
        let selected = select_templates(&all_options, &previous_selection)?;
        if selected.is_empty() {
            println!("No templates selected.");
            return Ok(());
        }

        // Update and save config
        update_and_save_config(&config_path, &mut config, &selected)?;

        // Ensure output directory exists
        ensure_output_directory(&output)?;

        // Generate gitignore content
        let content = generate_gitignore_content(&selected, index, &config)?;
        fs::write(&output, content)
            .with_context(|| format!("writing output file {}", output.display()))?;

        print_success_message(&output)?;
        Ok(())
    }
}
