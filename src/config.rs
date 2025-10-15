use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

// Security limits
pub const MAX_CUSTOM_TEMPLATE_SIZE: usize = 100 * 1024; // 100KB
pub const MAX_CUSTOM_TEMPLATE_LINES: usize = 10000;

#[derive(Deserialize, Serialize, Debug, Default)]
pub struct LignoreConfig {
    #[serde(default)]
    pub templates: Vec<String>,
    #[serde(default)]
    pub custom: BTreeMap<String, Vec<String>>,
}

/// Loads config or returns default if file doesn't exist
pub fn load_or_default_config(config_path: &PathBuf) -> LignoreConfig {
    if config_path.exists() {
        load_config(config_path).unwrap_or_default()
    } else {
        LignoreConfig::default()
    }
}

/// Validates configuration
pub fn validate_config(options: &[String], config: &LignoreConfig) -> Result<()> {
    check_invalid_templates(options, config).context("Invalid template configuration")?;
    check_shadowed_templates(options, config).context("Template name conflict detected")?;
    Ok(())
}

/// Builds the complete options list from official and custom templates
pub fn build_options_list(options: &[String], config: &LignoreConfig) -> Vec<String> {
    let mut all_options = Vec::new();
    let mut seen = BTreeSet::new();

    for custom_name in config.custom.keys() {
        if seen.insert(custom_name.clone()) {
            all_options.push(custom_name.clone());
        }
    }

    for template in &config.templates {
        if options.contains(template) && seen.insert(template.clone()) {
            all_options.push(template.clone());
        }
    }

    for template in options {
        if seen.insert(template.clone()) {
            all_options.push(template.clone());
        }
    }

    all_options
}

/// Builds previous selection list
pub fn build_previous_selection(options: &[String], config: &LignoreConfig) -> Vec<String> {
    let mut previous_selection: Vec<String> = config
        .templates
        .iter()
        .filter(|template| options.contains(template))
        .cloned()
        .collect();

    // Add all custom template names to previous selection (auto-check custom templates)
    previous_selection.extend(config.custom.keys().cloned());
    previous_selection
}

/// Updates and saves configuration
pub fn update_and_save_config(
    config_path: &PathBuf,
    config: &mut LignoreConfig,
    selected: &[String],
) -> Result<()> {
    config.templates = selected
        .iter()
        .filter(|template| !config.custom.contains_key(*template))
        .cloned()
        .collect();
    save_config(config_path, config)
}

fn load_config(path: &PathBuf) -> Result<LignoreConfig> {
    let content = fs::read_to_string(path)?;

    // Try to parse as new format first
    if let Ok(config) = serde_json::from_str::<LignoreConfig>(&content) {
        for (name, lines) in &config.custom {
            validate_custom_template(name, lines)
                .with_context(|| format!("validating custom template '{}'", name))?;
        }
        return Ok(config);
    }

    // Fall back to old format (simple array)
    if let Ok(templates) = serde_json::from_str::<Vec<String>>(&content) {
        return Ok(LignoreConfig {
            templates,
            custom: BTreeMap::new(),
        });
    }

    anyhow::bail!("Failed to parse lignore.json")
}

fn save_config(path: &PathBuf, config: &LignoreConfig) -> Result<()> {
    let content = serde_json::to_string_pretty(config)?;
    fs::write(path, content)?;
    Ok(())
}

/// Validates custom template content
pub fn validate_custom_template(name: &str, lines: &[String]) -> Result<()> {
    if lines.len() > MAX_CUSTOM_TEMPLATE_LINES {
        anyhow::bail!(
            "Custom template '{}' has too many lines: {} (max: {})",
            name,
            lines.len(),
            MAX_CUSTOM_TEMPLATE_LINES
        );
    }

    let total_size: usize = lines.iter().map(|l| l.len()).sum();
    if total_size > MAX_CUSTOM_TEMPLATE_SIZE {
        anyhow::bail!(
            "Custom template '{}' is too large: {} bytes (max: {} bytes)",
            name,
            total_size,
            MAX_CUSTOM_TEMPLATE_SIZE
        );
    }

    for (i, line) in lines.iter().enumerate() {
        if line.contains('\0') {
            anyhow::bail!(
                "Custom template '{}' contains null byte at line {}",
                name,
                i + 1
            );
        }
    }

    Ok(())
}

/// Checks for custom templates that shadow official templates and returns an error if found
fn check_shadowed_templates(official_templates: &[String], config: &LignoreConfig) -> Result<()> {
    // Build a map of lowercase official template names to their original names
    let official_lowercase: BTreeMap<String, Vec<String>> = {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for template in official_templates {
            map.entry(template.to_lowercase())
                .or_insert_with(Vec::new)
                .push(template.clone());
        }
        map
    };

    let mut shadowed: Vec<(String, String)> = Vec::new();

    for custom_name in config.custom.keys() {
        let custom_lower = custom_name.to_lowercase();
        if let Some(official_names) = official_lowercase.get(&custom_lower) {
            // Find the exact match or use the first official name
            let official_name = official_names
                .iter()
                .find(|name| *name == custom_name)
                .or_else(|| official_names.first())
                .unwrap();
            shadowed.push((custom_name.clone(), official_name.clone()));
        }
    }

    if !shadowed.is_empty() {
        let mut error_msg = String::from("Custom templates conflict with official templates:\n");
        for (custom_name, official_name) in &shadowed {
            if custom_name == official_name {
                error_msg.push_str(&format!("  - {} (exact match)\n", custom_name));
            } else {
                error_msg.push_str(&format!(
                    "  - {} (conflicts with: {})\n",
                    custom_name, official_name
                ));
            }
        }
        error_msg.push_str(
            "\nPlease rename your custom templates to avoid conflicts with official templates.",
        );
        anyhow::bail!(error_msg);
    }

    Ok(())
}

/// Checks for invalid template references and returns an error if found
fn check_invalid_templates(available_templates: &[String], config: &LignoreConfig) -> Result<()> {
    let invalid_templates: Vec<_> = config
        .templates
        .iter()
        .filter(|template| {
            !available_templates.contains(template) && !config.custom.contains_key(*template)
        })
        .cloned()
        .collect();

    if !invalid_templates.is_empty() {
        let mut error_msg = String::from("The following templates in lignore.json do not exist:\n");
        for template in &invalid_templates {
            error_msg.push_str(&format!("  - {}\n", template));
        }
        error_msg.push_str("\nRun `lignore list` to see available templates or define them in the 'custom' section.");
        anyhow::bail!(error_msg);
    }

    Ok(())
}
