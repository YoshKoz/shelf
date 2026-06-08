use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::models::Item;

/// Build a safe filename from a title.
fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    for c in title.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            slug.extend(c.to_lowercase());
        } else if c.is_whitespace() {
            slug.push('-');
        } else {
            // skip non-alphanumeric, non-safe characters (except we keep . and -)
            if c == '.' || c == '/' {
                slug.push('-');
            }
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

/// Generate a unique filename for a new item inside `data_dir`.
pub fn fresh_filename(data_dir: &Path, title: &str) -> PathBuf {
    let base = slugify(title);
    let base = if base.len() > 80 {
        &base[..80]
    } else {
        &base
    };
    let candidate = data_dir.join(format!("{}.md", base));
    if !candidate.exists() {
        return candidate;
    }
    // Collision – append a counter.
    for i in 1..100 {
        let candidate = data_dir.join(format!("{}-{}.md", base, i));
        if !candidate.exists() {
            return candidate;
        }
    }
    // Last resort: timestamp.
    let ts = chrono::Local::now().format("%Y%m%d%H%M%S");
    data_dir.join(format!("{}-{}.md", base, ts))
}

/// List all `.md` files in `data_dir`.
pub fn list_files(data_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !data_dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(data_dir).context("reading data directory")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            files.push(path);
        }
    }
    files.sort(); // deterministic order
    Ok(files)
}

/// Read a single `.md` file and parse it into an `Item`.
pub fn read_item(path: &Path) -> anyhow::Result<Item> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown.md")
        .to_string();
    Item::from_markdown(&file_name, &raw)
}

/// Write an `Item` to a `.md` file in `data_dir`.
pub fn write_item(item: &Item, data_dir: &Path) -> anyhow::Result<PathBuf> {
    let path = data_dir.join(&item.file_name);
    let md = item.to_markdown();
    std::fs::write(&path, &md)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Delete an item's `.md` file.
pub fn delete_item(file_name: &str, data_dir: &Path) -> anyhow::Result<()> {
    let path = data_dir.join(file_name);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}

/// Build a file path from its name.
pub fn item_path(data_dir: &Path, file_name: &str) -> PathBuf {
    data_dir.join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Rust & Stuff!!"), "rust--stuff");
        assert_eq!(slugify("  spaces  "), "spaces");
        assert_eq!(slugify(""), "untitled");
    }
}
