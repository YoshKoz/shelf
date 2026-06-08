use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    Bookmark,
    Note,
}

impl std::fmt::Display for ItemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ItemType::Bookmark => write!(f, "bookmark"),
            ItemType::Note => write!(f, "note"),
        }
    }
}

impl Default for ItemType {
    fn default() -> Self {
        ItemType::Note
    }
}

/// The YAML frontmatter embedded in each .md file.
#[derive(Debug, Serialize, Deserialize)]
struct Frontmatter {
    title: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(rename = "type", default)]
    item_type: ItemType,
    created: NaiveDate,
    modified: NaiveDate,
}

/// A single shelf item – a bookmark or note.
#[derive(Debug, Clone)]
pub struct Item {
    pub id: Option<i64>,
    pub file_name: String,
    pub title: String,
    pub tags: Vec<String>,
    pub url: Option<String>,
    pub item_type: ItemType,
    pub created: NaiveDate,
    pub modified: NaiveDate,
    /// Markdown body (everything after the frontmatter).
    pub content: String,
}

impl Item {
    /// Serialise this item to a markdown string with YAML frontmatter.
    pub fn to_markdown(&self) -> String {
        let tags_yaml = if self.tags.is_empty() {
            "[]".to_string()
        } else {
            let items: Vec<String> = self.tags.iter().map(|t| format!("\"{}\"", t)).collect();
            format!("[{}]", items.join(", "))
        };

        let url_line = match &self.url {
            Some(u) => format!("\nurl: \"{}\"", u),
            None => String::new(),
        };

        format!(
            "---\ntitle: \"{}\"\ntags: {}\ntype: {}{}\ncreated: {}\nmodified: {}\n---\n\n{}",
            self.title,
            tags_yaml,
            self.item_type,
            url_line,
            self.created,
            self.modified,
            self.content
        )
    }

    /// Parse an item from a markdown string. The `file_name` is provided externally.
    pub fn from_markdown(file_name: &str, raw: &str) -> anyhow::Result<Self> {
        let (fm, body) = parse_frontmatter(raw)?;
        Ok(Item {
            id: None,
            file_name: file_name.to_string(),
            title: fm.title,
            tags: fm.tags,
            url: fm.url,
            item_type: fm.item_type,
            created: fm.created,
            modified: fm.modified,
            content: body,
        })
    }
}

/// Split raw text into (Frontmatter, body).  Returns a default frontmatter + full
/// text as body when no `---` fence is found.
fn parse_frontmatter(raw: &str) -> anyhow::Result<(Frontmatter, String)> {
    let raw = raw.trim_start();
    if !raw.starts_with("---") {
        // No frontmatter – treat whole file as body, synthesise a title from the
        // first line.
        let body = raw.trim().to_string();
        let title = body.lines().next().unwrap_or("untitled").to_string();
        let today = chrono::Local::now().date_naive();
        return Ok((
            Frontmatter {
                title,
                tags: vec![],
                url: None,
                item_type: ItemType::Note,
                created: today,
                modified: today,
            },
            body,
        ));
    }

    let after_first = &raw[3..];
    let end = after_first
        .find("\n---")
        .or_else(|| after_first.find("\n---\n"))
        .ok_or_else(|| anyhow::anyhow!("unclosed frontmatter fence"))?;

    let yaml_str = &after_first[..end];
    let body_start = end + 4; // skip "\n---" + possible "\n"
    let body = if body_start < after_first.len() {
        after_first[body_start..].trim().to_string()
    } else {
        String::new()
    };

    let fm: Frontmatter = serde_yaml::from_str(yaml_str)
        .map_err(|e| anyhow::anyhow!("frontmatter parse error: {e}"))?;

    Ok((fm, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let item = Item {
            id: None,
            file_name: "test.md".into(),
            title: "Hello World".into(),
            tags: vec!["rust".into(), "test".into()],
            url: Some("https://example.com".into()),
            item_type: ItemType::Bookmark,
            created: NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
            modified: NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
            content: "Some **markdown** body.".into(),
        };
        let md = item.to_markdown();
        let parsed = Item::from_markdown("test.md", &md).unwrap();
        assert_eq!(parsed.title, item.title);
        assert_eq!(parsed.tags, item.tags);
        assert_eq!(parsed.url, item.url);
        assert_eq!(parsed.item_type, item.item_type);
        assert_eq!(parsed.content, item.content);
    }

    #[test]
    fn no_frontmatter() {
        let raw = "Just a bare note\n\nwith some text.";
        let item = Item::from_markdown("bare.md", raw).unwrap();
        assert_eq!(item.title, "Just a bare note");
        assert_eq!(item.item_type, ItemType::Note);
    }
}
