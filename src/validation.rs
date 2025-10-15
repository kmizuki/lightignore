use anyhow::Result;
use std::path::Path;

/// Validates template key to prevent path traversal attacks
pub fn validate_template_key(key: &str) -> Result<()> {
    if key.is_empty() {
        anyhow::bail!("Template key cannot be empty");
    }

    if key.contains("..") {
        anyhow::bail!("Template key contains invalid sequence: ..");
    }

    if key.starts_with('/') || key.starts_with('\\') {
        anyhow::bail!("Template key cannot start with path separator");
    }

    if key.contains('\\') {
        anyhow::bail!("Template key contains invalid character: \\");
    }

    if key.contains('\0') {
        anyhow::bail!("Template key contains null byte");
    }

    if key.len() > 255 {
        anyhow::bail!("Template key is too long (max: 255 characters)");
    }

    Ok(())
}

/// Validates output path to prevent writing to dangerous locations
pub fn validate_output_path(path: &Path) -> Result<()> {
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    // Normalize path (resolve .. and .)
    let canonical_path = abs_path.canonicalize().unwrap_or(abs_path.clone());

    // Check if path tries to escape current directory
    let current_dir = std::env::current_dir()?;
    if !canonical_path.starts_with(&current_dir) {
        // Allow writing to absolute paths, but warn about suspicious patterns
        if path.to_string_lossy().contains("..") {
            anyhow::bail!("Output path contains suspicious pattern: ..");
        }
    }

    // Prevent writing to system directories
    let path_str = canonical_path.to_string_lossy();
    let dangerous_paths = [
        "/etc/",
        "/sys/",
        "/proc/",
        "/dev/",
        "/boot/",
        "/bin/",
        "/sbin/",
        "/usr/bin/",
        "/usr/sbin/",
    ];

    for dangerous in &dangerous_paths {
        if path_str.starts_with(dangerous) {
            anyhow::bail!("Cannot write to system directory: {}", dangerous);
        }
    }

    Ok(())
}
