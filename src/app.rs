use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::db;
pub use crate::models::{Item, ItemType};
use crate::storage;

/// Which screen is the user on.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    Search,
    Create,
    Edit,
    ConfirmDelete,
}

/// Focus within the create/edit form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FormField {
    Title,
    Url,
    Tags,
    TypeToggle,
    Content,
}

impl FormField {
    #[allow(dead_code)]
    pub const ALL: [FormField; 5] = [
        FormField::Title,
        FormField::Url,
        FormField::Tags,
        FormField::TypeToggle,
        FormField::Content,
    ];

    pub fn next(self) -> Self {
        match self {
            FormField::Title => FormField::Url,
            FormField::Url => FormField::Tags,
            FormField::Tags => FormField::TypeToggle,
            FormField::TypeToggle => FormField::Content,
            FormField::Content => FormField::Title,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            FormField::Title => FormField::Content,
            FormField::Url => FormField::Title,
            FormField::Tags => FormField::Url,
            FormField::TypeToggle => FormField::Tags,
            FormField::Content => FormField::TypeToggle,
        }
    }
}

/// Temporary form state for creating or editing an item.
#[derive(Debug, Clone)]
pub struct Form {
    pub title: String,
    pub url: String,
    pub tags: String,
    pub content: String,
    pub item_type: ItemType,
    pub field: FormField,
    pub is_edit: bool,
    pub original_file: String,
}

impl Form {
    pub fn new(item_type: ItemType) -> Self {
        Self {
            title: String::new(),
            url: String::new(),
            tags: String::new(),
            content: String::new(),
            item_type,
            field: FormField::Title,
            is_edit: false,
            original_file: String::new(),
        }
    }

    pub fn from_item(item: &Item) -> Self {
        Self {
            title: item.title.clone(),
            url: item.url.clone().unwrap_or_default(),
            tags: item.tags.join(", "),
            content: item.content.clone(),
            item_type: item.item_type.clone(),
            field: FormField::Title,
            is_edit: true,
            original_file: item.file_name.clone(),
        }
    }

    pub fn to_item(&self, file_name: String) -> Item {
        let today = chrono::Local::now().date_naive();
        Item {
            id: None,
            file_name,
            title: self.title.clone(),
            tags: self
                .tags
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            url: if self.url.trim().is_empty() {
                None
            } else {
                Some(self.url.trim().to_string())
            },
            item_type: self.item_type.clone(),
            created: today,
            modified: today,
            content: self.content.clone(),
        }
    }

    pub fn validate(&self) -> Option<String> {
        if self.title.trim().is_empty() {
            return Some("Title cannot be empty".into());
        }
        None
    }
}

/// The main application state.
pub struct App {
    pub mode: Mode,
    pub items: Vec<Item>,
    pub selected: usize,
    pub detail_scroll: usize,
    pub search_query: String,
    pub form: Option<Form>,
    pub status: String,
    pub should_quit: bool,
    pub needs_rebuild: bool,

    // Internals
    _db_path: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    conn: rusqlite::Connection,
    matcher: SkimMatcherV2,
}

impl App {
    pub fn new(
        db_path: std::path::PathBuf,
        data_dir: std::path::PathBuf,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        let conn = db::open_or_create(&db_path)?;

        let mut app = Self {
            mode: Mode::Normal,
            items: vec![],
            selected: 0,
            detail_scroll: 0,
            search_query: String::new(),
            form: None,
            status: String::new(),
            should_quit: false,
            needs_rebuild: false,
            _db_path: db_path,
            data_dir,
            conn,
            matcher: SkimMatcherV2::default(),
        };

        app.rebuild_index()?;
        app.refresh_items()?;
        Ok(app)
    }

    /// Scan the filesystem and sync the database.
    pub fn rebuild_index(&mut self) -> anyhow::Result<()> {
        let files = storage::list_files(&self.data_dir)?;
        let mut items = Vec::with_capacity(files.len());
        for path in &files {
            match storage::read_item(path) {
                Ok(item) => items.push(item),
                Err(e) => {
                    eprintln!("warning: could not parse {}: {e}", path.display());
                }
            }
        }
        let (added, updated) = db::reconcile(&self.conn, &items)?;
        if added > 0 || updated > 0 {
            self.status = format!("Index: {} added, {} updated", added, updated);
        }
        self.needs_rebuild = false;
        Ok(())
    }

    /// Reload items from the database (respecting current search filter).
    pub fn refresh_items(&mut self) -> anyhow::Result<()> {
        let query = self.search_query.trim();
        self.items = if query.is_empty() {
            db::list_all(&self.conn)?
        } else {
            // Try FTS first
            let results = db::search_fts(&self.conn, query)?;
            if !results.is_empty() {
                results
            } else {
                // Fallback: fuzzy match on all items
                let all = db::list_all(&self.conn)?;
                self.fuzzy_filter(all, query)
            }
        };
        // Clamp selection
        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
        Ok(())
    }

    fn fuzzy_filter(&self, items: Vec<Item>, query: &str) -> Vec<Item> {
        let mut scored: Vec<(i64, Item)> = items
            .into_iter()
            .filter_map(|item| {
                let haystack = format!(
                    "{} {} {}",
                    item.title,
                    item.tags.join(" "),
                    item.content
                );
                let score = self.matcher.fuzzy_match(&haystack, query)?;
                // Boost title matches
                let title_score = self
                    .matcher
                    .fuzzy_match(&item.title, query)
                    .unwrap_or(0);
                Some((score + title_score * 2, item))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, item)| item).collect()
    }

    // ------------------------------------------------------------------
    // Actions
    // ------------------------------------------------------------------

    pub fn start_search(&mut self) {
        self.mode = Mode::Search;
        self.search_query.clear();
    }

    pub fn commit_search(&mut self) {
        self.mode = Mode::Normal;
        if let Err(e) = self.refresh_items() {
            self.status = format!("Search error: {e}");
        }
        self.status = format!(
            "{} items{}",
            self.items.len(),
            if self.search_query.trim().is_empty() {
                String::new()
            } else {
                format!(" matching \"{}\"", self.search_query.trim())
            }
        );
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.mode = Mode::Normal;
        if let Err(e) = self.refresh_items() {
            self.status = format!("Error: {e}");
        }
    }

    pub fn start_create(&mut self) {
        let form = Form::new(ItemType::Note);
        self.form = Some(form);
        self.mode = Mode::Create;
    }

    pub fn start_edit(&mut self) -> anyhow::Result<()> {
        if self.items.is_empty() {
            return Ok(());
        }
        let item = &self.items[self.selected];
        let form = Form::from_item(item);
        self.form = Some(form);
        self.mode = Mode::Edit;
        Ok(())
    }

    pub fn save_form(&mut self) -> anyhow::Result<()> {
        let form = self.form.take().expect("form exists");

        if let Some(err) = form.validate() {
            self.status = err;
            self.form = Some(form);
            return Ok(());
        }

        if form.is_edit {
            // Update existing item
            let file_name = form.original_file.clone();
            let mut item = form.to_item(file_name.clone());
            // Preserve original created date
            if let Ok(old) = storage::read_item(&storage::item_path(&self.data_dir, &file_name))
            {
                item.created = old.created;
            } else if let Some(db_item) = self.items.iter().find(|i| i.file_name == file_name) {
                item.created = db_item.created;
            }
            item.modified = chrono::Local::now().date_naive();
            item.file_name = file_name.clone();

            storage::write_item(&item, &self.data_dir)?;
            db::update_item(&self.conn, &item)?;
            self.status = format!("Updated \"{}\"", item.title);
        } else {
            // Create new item
            let path = storage::fresh_filename(&self.data_dir, &form.title);
            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("untitled.md")
                .to_string();
            let mut item = form.to_item(file_name);
            item.created = chrono::Local::now().date_naive();
            item.modified = chrono::Local::now().date_naive();

            storage::write_item(&item, &self.data_dir)?;
            let id = db::insert_item(&self.conn, &item)?;
            item.id = Some(id);
            self.status = format!("Created \"{}\"", item.title);
        }

        self.mode = Mode::Normal;
        self.refresh_items()?;
        Ok(())
    }

    pub fn cancel_form(&mut self) {
        self.form = None;
        self.mode = Mode::Normal;
    }

    pub fn start_delete(&mut self) {
        if !self.items.is_empty() {
            self.mode = Mode::ConfirmDelete;
        }
    }

    pub fn confirm_delete(&mut self) -> anyhow::Result<()> {
        if self.items.is_empty() {
            self.mode = Mode::Normal;
            return Ok(());
        }
        let item = self.items.remove(self.selected);
        if self.selected >= self.items.len() && !self.items.is_empty() {
            self.selected = self.items.len() - 1;
        }

        storage::delete_item(&item.file_name, &self.data_dir)?;
        db::delete_item(&self.conn, &item.file_name)?;
        self.status = format!("Deleted \"{}\"", item.title);
        self.mode = Mode::Normal;
        if self.items.is_empty() {
            self.selected = 0;
        }
        Ok(())
    }

    pub fn cancel_delete(&mut self) {
        self.mode = Mode::Normal;
    }

    pub fn open_url(&self) {
        if self.items.is_empty() {
            return;
        }
        let item = &self.items[self.selected];
        if let Some(url) = &item.url {
            if let Err(e) = open::that(url) {
                eprintln!("Could not open URL: {e}");
            }
        }
    }

    pub fn next_item(&mut self) {
        if !self.items.is_empty() {
            let len = self.items.len();
            self.selected = (self.selected + 1) % len;
            self.detail_scroll = 0;
        }
    }

    pub fn prev_item(&mut self) {
        if !self.items.is_empty() {
            let len = self.items.len();
            self.selected = if self.selected == 0 {
                len - 1
            } else {
                self.selected - 1
            };
            self.detail_scroll = 0;
        }
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }
}
