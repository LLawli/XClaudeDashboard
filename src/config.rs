use std::path::{Path, PathBuf};

use color_eyre::Result;
use color_eyre::eyre::{WrapErr, eyre};

pub fn default_db_path() -> Result<PathBuf> {
    Ok(claude_dir()?.join("data").join("xclaude-usage.db"))
}

pub fn default_cloud_config() -> Result<PathBuf> {
    Ok(claude_dir()?.join("data").join("xclaude-cloud.json"))
}

pub fn default_prices_cache() -> Result<PathBuf> {
    Ok(claude_dir()?.join("data").join("xclaude-prices.json"))
}

fn claude_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre!("could not resolve home directory"))?;
    Ok(home.join(".claude"))
}

pub fn load_cloud_config(path: &Path) -> Result<serde_json::Value> {
    let raw = std::fs::read_to_string(path)
        .wrap_err_with(|| format!("reading cloud config at {}", path.display()))?;
    let value = serde_json::from_str(&raw)
        .wrap_err_with(|| format!("parsing cloud config at {}", path.display()))?;
    Ok(value)
}
