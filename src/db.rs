use std::path::Path;

use anyhow::Context;
use rusqlite::{params, Connection};

use crate::models::{Item, ItemType};

/// Initialise the database: create tables and FTS5 index if they don't exist.
pub fn open_or_create(db_path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("opening database {}", db_path.display()))?;

    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS items (
            id        INTEGER PRIMARY KEY AUTOINCREMENT,
            file_name TEXT    UNIQUE NOT NULL,
            title     TEXT    NOT NULL,
            tags      TEXT    NOT NULL DEFAULT '',
            url       TEXT,
            item_type TEXT    NOT NULL DEFAULT 'note',
            created   TEXT    NOT NULL,
            modified  TEXT    NOT NULL,
            content   TEXT    NOT NULL DEFAULT ''
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
            title, tags, content,
            content='items',
            content_rowid='id'
        );

        CREATE TRIGGER IF NOT EXISTS items_ai AFTER INSERT ON items BEGIN
            INSERT INTO items_fts(rowid, title, tags, content)
            VALUES (new.id, new.title, new.tags, new.content);
        END;

        CREATE TRIGGER IF NOT EXISTS items_ad AFTER DELETE ON items BEGIN
            INSERT INTO items_fts(items_fts, rowid, title, tags, content)
            VALUES('delete', old.id, old.title, old.tags, old.content);
        END;

        CREATE TRIGGER IF NOT EXISTS items_au AFTER UPDATE ON items BEGIN
            INSERT INTO items_fts(items_fts, rowid, title, tags, content)
            VALUES('delete', old.id, old.title, old.tags, old.content);
            INSERT INTO items_fts(rowid, title, tags, content)
            VALUES (new.id, new.title, new.tags, new.content);
        END;
        ",
    )?;
    Ok(conn)
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

pub fn insert_item(conn: &Connection, item: &Item) -> anyhow::Result<i64> {
    let tags_str = item.tags.join(",");
    conn.execute(
        "INSERT INTO items (file_name, title, tags, url, item_type, created, modified, content)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            item.file_name,
            item.title,
            tags_str,
            item.url,
            item.item_type.to_string(),
            item.created.to_string(),
            item.modified.to_string(),
            item.content,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_item(conn: &Connection, item: &Item) -> anyhow::Result<()> {
    let tags_str = item.tags.join(",");
    conn.execute(
        "UPDATE items SET title=?1, tags=?2, url=?3, item_type=?4, modified=?5, content=?6
         WHERE file_name=?7",
        params![
            item.title,
            tags_str,
            item.url,
            item.item_type.to_string(),
            item.modified.to_string(),
            item.content,
            item.file_name,
        ],
    )?;
    Ok(())
}

pub fn delete_item(conn: &Connection, file_name: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM items WHERE file_name=?1", params![file_name])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<Item> {
    let tags_str: String = row.get("tags")?;
    let tags: Vec<String> = if tags_str.is_empty() {
        vec![]
    } else {
        tags_str.split(',').map(|s| s.trim().to_string()).collect()
    };
    let type_str: String = row.get("item_type")?;
    let item_type = match type_str.as_str() {
        "bookmark" => ItemType::Bookmark,
        _ => ItemType::Note,
    };
    Ok(Item {
        id: Some(row.get("id")?),
        file_name: row.get("file_name")?,
        title: row.get("title")?,
        tags,
        url: row.get("url")?,
        item_type,
        created: row.get::<_, String>("created")?.parse().unwrap_or_default(),
        modified: row.get::<_, String>("modified")?.parse().unwrap_or_default(),
        content: row.get("content")?,
    })
}

/// Return all items ordered by most recently modified first.
pub fn list_all(conn: &Connection) -> anyhow::Result<Vec<Item>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_name, title, tags, url, item_type, created, modified, content
         FROM items
         ORDER BY modified DESC",
    )?;
    let rows = stmt
        .query_map([], row_to_item)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Full-text search via FTS5. The query string is augmented with `*` prefix
/// matching on each term.
pub fn search_fts(conn: &Connection, query: &str) -> anyhow::Result<Vec<Item>> {
    if query.trim().is_empty() {
        return list_all(conn);
    }

    // Build a prefix query: each word gets a * suffix.
    let fts_query: String = query
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|w| format!("{}*", w))
        .collect::<Vec<_>>()
        .join(" AND ");

    let mut stmt = conn.prepare(
        "SELECT i.id, i.file_name, i.title, i.tags, i.url, i.item_type,
                i.created, i.modified, i.content
         FROM items i
         JOIN items_fts fts ON i.id = fts.rowid
         WHERE items_fts MATCH ?1
         ORDER BY rank
         LIMIT 200",
    )?;

    let rows = stmt
        .query_map(params![fts_query], row_to_item)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Reconciliation: scan filesystem and bring DB in sync
// ---------------------------------------------------------------------------

/// Make the database match the files on disk.  New files are inserted,
/// changed files are updated, files that disappeared are *kept* (the files
/// are the source of truth – we don't auto-delete from DB when a file is
/// removed, so the user can restore from trash).
pub fn reconcile(
    conn: &Connection,
    items: &[Item],
) -> anyhow::Result<(usize, usize)> {
    let mut added = 0;
    let mut updated = 0;

    // Build a map of known DB entries by file_name.
    let existing: std::collections::HashMap<String, String> = {
        let mut stmt = conn.prepare(
            "SELECT file_name, modified FROM items",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().collect()
    };

    for item in items {
        match existing.get(&item.file_name) {
            None => {
                insert_item(conn, item)?;
                added += 1;
            }
            Some(db_modified) => {
                let item_modified = item.modified.to_string();
                if item_modified != *db_modified {
                    update_item(conn, item)?;
                    updated += 1;
                }
            }
        }
    }

    Ok((added, updated))
}
