# shelf

A local-first TUI for bookmarks and notes — plain markdown files on disk, a SQLite index on top for fast full-text search. No cloud, no account, no server.

## Why

Bookmark managers either lock your data in a proprietary DB or a browser profile, or they're a web app with a login. `shelf` keeps every item as a readable `.md` file with YAML frontmatter in your XDG data dir — the SQLite database (with FTS5) is a disposable, rebuildable index over those files, not the source of truth. Delete the DB and `shelf` just reindexes from the markdown on next launch.

## Features

- Add bookmarks or free-form notes from a keyboard-driven TUI ([ratatui](https://ratatui.rs))
- Fuzzy search across title/tags/content (`fuzzy-matcher`) live as you type, plus SQLite FTS5 full-text search
- Items stored as individual markdown files with YAML frontmatter (title, tags, url, type, created/modified dates) — human-readable, grep-able, versionable
- Open a bookmark's URL directly from the list, or yank it to the clipboard
- Create / edit / delete with an in-TUI form, delete requires confirmation
- Export the whole collection to Markdown, HTML, or JSON: `shelf export --format json`

## Install / run

```bash
cargo build --release
./target/release/shelf              # launch the TUI
./target/release/shelf export --format html > bookmarks.html
```

Data lives under the platform's XDG data dir (e.g. `~/.local/share/shelf` on Linux, `%LOCALAPPDATA%\shelf` on Windows) as one `.md` file per item, indexed into `shelf.db`.

## Keybindings

| Key | Action |
|---|---|
| `j` / `k`, ↑ / ↓ | move selection |
| `/` | search |
| `n` | new item |
| `e` | edit selected |
| `d` | delete selected (confirm with `y`) |
| `o` / Enter | open URL |
| `y` | yank URL to clipboard |
| `g` / `G` | jump to top / bottom |
| `Ctrl+C` / `Ctrl+Q` | quit |

## Project layout

```
src/main.rs     CLI entry, terminal setup/teardown, key-event dispatch
src/app.rs      App state, modes (Normal/Search/Create/Edit/ConfirmDelete)
src/ui.rs       ratatui rendering
src/models.rs   Item/ItemType, markdown <-> frontmatter (de)serialization
src/db.rs       SQLite schema + FTS5 index/triggers
src/storage.rs  markdown file I/O, filename slugging/collision handling
```

## Tech stack

Rust, [ratatui](https://ratatui.rs) + crossterm for the terminal UI, `rusqlite` (bundled SQLite with FTS5), `serde_yaml` for frontmatter, `chrono`, `directories` for XDG paths.
