use crate::ui::theme::get_theme;
use anyhow::Result;
use crossterm::{
    ExecutableCommand, QueueableCommand,
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::cmp::{max, min};
use std::collections::BTreeSet;
use std::io::{Stdout, Write, stdout};

pub fn select_templates(
    options: &[String],
    previous_selection: &[String],
) -> Result<Option<Vec<String>>> {
    if options.is_empty() {
        return Ok(Some(Vec::new()));
    }

    let mut guard = TerminalGuard::enter()?;
    let mut state = SelectionState::new(options.to_vec());

    for (idx, item) in options.iter().enumerate() {
        if previous_selection.contains(item) {
            state.select_item(idx);
        }
    }

    let result = loop {
        state.render(guard.stdout_mut())?;
        guard.stdout_mut().flush()?;

        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if state.handle_search_key(&key) {
                    continue;
                }

                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        break Ok(None);
                    }
                    KeyCode::Enter => break Ok(Some(state.finish())),
                    KeyCode::Char(' ') | KeyCode::Char('　') => {
                        state.toggle_current();
                    }
                    KeyCode::Up | KeyCode::Char('k') => state.move_up(),
                    KeyCode::Down | KeyCode::Char('j') => state.move_down(),
                    KeyCode::Left | KeyCode::Char('h') => state.move_left(),
                    KeyCode::Right | KeyCode::Char('l') => state.move_right(),
                    KeyCode::PageUp => state.page_up(),
                    KeyCode::PageDown => state.page_down(),
                    KeyCode::Home => state.move_home(),
                    KeyCode::End => state.move_end(),
                    KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.select_all()
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        state.clear_all()
                    }
                    _ => {}
                }
            }
            Event::Resize(_, _) => state.invalidate_cache(),
            _ => {}
        }
    };

    guard.exit()?;
    result
}

pub struct SelectionState {
    items: Vec<String>,
    filtered_indices: Vec<usize>,
    selected: BTreeSet<usize>,
    cursor: usize,
    viewport_offset: usize,
    cached_layout: Option<Layout>,
    search_query: String,
    search_active: bool,
}

#[derive(Clone)]
struct Layout {
    columns: usize,
    column_width: usize,
    rows_visible: usize,
}

impl SelectionState {
    pub fn new(items: Vec<String>) -> Self {
        let mut state = Self {
            items,
            filtered_indices: Vec::new(),
            selected: BTreeSet::new(),
            cursor: 0,
            viewport_offset: 0,
            cached_layout: None,
            search_query: String::new(),
            search_active: false,
        };
        state.refresh_filter(true);
        state
    }

    pub fn invalidate_cache(&mut self) {
        self.cached_layout = None;
    }

    fn refresh_filter(&mut self, reset_position: bool) {
        if self.search_query.is_empty() {
            self.filtered_indices = (0..self.items.len()).collect();
        } else {
            let needle = self.search_query.to_lowercase();
            self.filtered_indices = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(idx, item)| {
                    if item.to_lowercase().contains(&needle) {
                        Some(idx)
                    } else {
                        None
                    }
                })
                .collect();
        }

        if reset_position {
            self.cursor = 0;
            self.viewport_offset = 0;
        } else if let Some(last_index) = self.filtered_indices.len().checked_sub(1) {
            self.cursor = min(self.cursor, last_index);
            self.viewport_offset = min(self.viewport_offset, last_index);
        } else {
            self.cursor = 0;
            self.viewport_offset = 0;
        }

        self.invalidate_cache();
    }

    fn total_count(&self) -> usize {
        self.items.len()
    }

    fn visible_count(&self) -> usize {
        self.filtered_indices.len()
    }

    fn current_item_index(&self) -> Option<usize> {
        self.filtered_indices.get(self.cursor).copied()
    }

    fn filter_matches_full_list(&self) -> bool {
        self.visible_count() == self.total_count()
    }

    pub fn enter_search_mode(&mut self) {
        self.search_active = true;
    }

    pub fn exit_search_mode(&mut self) {
        self.search_active = false;
    }

    pub fn push_search_char(&mut self, ch: char) {
        self.search_query.push(ch);
        self.refresh_filter(true);
    }

    pub fn pop_search_char(&mut self) {
        self.search_query.pop();
        self.refresh_filter(true);
        if self.search_query.is_empty() {
            self.search_active = false;
        }
    }

    pub fn clear_search(&mut self) {
        if !self.search_query.is_empty() {
            self.search_query.clear();
            self.refresh_filter(true);
        }
        self.search_active = false;
    }

    fn is_typable_char(ch: char, modifiers: KeyModifiers) -> bool {
        !ch.is_control() && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT)
    }

    fn is_reserved_hotkey(ch: char) -> bool {
        matches!(ch, 'q' | 'j' | 'k' | 'h' | 'l' | ' ')
    }

    pub fn handle_search_key(&mut self, key: &KeyEvent) -> bool {
        if self.search_active {
            match key.code {
                KeyCode::Esc => {
                    self.clear_search();
                    return true;
                }
                KeyCode::Backspace => {
                    if self.search_query.is_empty() {
                        self.exit_search_mode();
                    } else {
                        self.pop_search_char();
                    }
                    return true;
                }
                KeyCode::Delete => {
                    if !self.search_query.is_empty() {
                        self.clear_search();
                    }
                    return true;
                }
                KeyCode::Enter => {
                    self.exit_search_mode();
                    return false;
                }
                KeyCode::Char(' ') | KeyCode::Char('　') if key.modifiers.is_empty() => {
                    return false;
                }
                KeyCode::Char(ch) if Self::is_typable_char(ch, key.modifiers) => {
                    self.push_search_char(ch);
                    return true;
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Char('/') if key.modifiers.is_empty() => {
                    self.enter_search_mode();
                    return true;
                }
                KeyCode::Char(ch)
                    if Self::is_typable_char(ch, key.modifiers)
                        && !Self::is_reserved_hotkey(ch) =>
                {
                    self.search_query.clear();
                    self.enter_search_mode();
                    self.push_search_char(ch);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn layout(&mut self) -> Result<Layout> {
        if let Some(layout) = &self.cached_layout {
            return Ok(layout.clone());
        }

        let (width, height) = terminal::size()?;
        let max_item_width = self
            .filtered_indices
            .iter()
            .map(|&idx| self.items[idx].len())
            .max()
            .unwrap_or(0)
            + 4;
        let term_width = width.saturating_sub(2) as usize;
        let mut columns = max(1, term_width / max_item_width.max(1));
        columns = min(columns, self.visible_count().max(1));
        let rows_visible = max(1, height.saturating_sub(5) as usize);

        let layout = Layout {
            columns,
            column_width: max_item_width,
            rows_visible,
        };
        self.cached_layout = Some(layout.clone());
        Ok(layout)
    }

    fn ensure_visible(&mut self, layout: &Layout) {
        let visible = self.visible_count();
        if visible == 0 {
            self.cursor = 0;
            self.viewport_offset = 0;
            return;
        }

        if self.cursor >= visible {
            self.cursor = visible - 1;
        }

        let viewport_capacity = layout.columns.max(1) * layout.rows_visible;
        if viewport_capacity == 0 {
            self.viewport_offset = 0;
            return;
        }

        // Calculate which "page" the cursor is on (in row-major order)
        let cursor_page = self.cursor / viewport_capacity;
        let viewport_page = self.viewport_offset / viewport_capacity;

        // If cursor is on a different page, adjust viewport
        if cursor_page != viewport_page {
            self.viewport_offset = cursor_page * viewport_capacity;
        }

        // Ensure viewport_offset doesn't exceed the valid range
        let max_offset = if visible <= viewport_capacity {
            0
        } else {
            ((visible - 1) / viewport_capacity) * viewport_capacity
        };

        if self.viewport_offset > max_offset {
            self.viewport_offset = max_offset;
        }
    }

    pub fn move_up(&mut self) {
        if let Ok(layout) = self.layout() {
            if self.cursor >= layout.columns {
                self.cursor -= layout.columns;
            }
            self.ensure_visible(&layout);
        }
    }

    pub fn move_down(&mut self) {
        if let Ok(layout) = self.layout() {
            let visible = self.visible_count();
            if visible == 0 {
                return;
            }
            if self.cursor + layout.columns < visible {
                self.cursor += layout.columns;
            } else {
                self.cursor = visible - 1;
            }
            self.ensure_visible(&layout);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        if let Ok(layout) = self.layout() {
            self.ensure_visible(&layout);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor + 1 < self.visible_count() {
            self.cursor += 1;
        }
        if let Ok(layout) = self.layout() {
            self.ensure_visible(&layout);
        }
    }

    pub fn page_up(&mut self) {
        if let Ok(layout) = self.layout() {
            let step = layout.rows_visible * layout.columns.max(1);
            self.cursor = self.cursor.saturating_sub(step);
            self.ensure_visible(&layout);
        }
    }

    pub fn page_down(&mut self) {
        if let Ok(layout) = self.layout() {
            let step = layout.rows_visible * layout.columns.max(1);
            let visible = self.visible_count();
            if visible == 0 {
                return;
            }
            if self.cursor + step < visible {
                self.cursor += step;
            } else {
                self.cursor = visible.saturating_sub(1);
            }
            self.ensure_visible(&layout);
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
        if let Ok(layout) = self.layout() {
            self.ensure_visible(&layout);
        }
    }

    pub fn move_end(&mut self) {
        let visible = self.visible_count();
        if visible > 0 {
            self.cursor = visible - 1;
            if let Ok(layout) = self.layout() {
                self.ensure_visible(&layout);
            }
        }
    }

    pub fn toggle_current(&mut self) {
        if let Some(idx) = self.current_item_index() {
            if self.selected.contains(&idx) {
                self.selected.remove(&idx);
            } else {
                self.selected.insert(idx);
            }
        }
    }

    pub fn select_all(&mut self) {
        if self.filter_matches_full_list() {
            self.selected.clear();
        }
        for idx in &self.filtered_indices {
            self.selected.insert(*idx);
        }
    }

    pub fn clear_all(&mut self) {
        if self.filter_matches_full_list() {
            self.selected.clear();
        } else {
            for idx in &self.filtered_indices {
                self.selected.remove(idx);
            }
        }
    }

    pub fn select_item(&mut self, idx: usize) {
        if idx < self.items.len() {
            self.selected.insert(idx);
        }
    }

    pub fn finish(self) -> Vec<String> {
        self.selected
            .into_iter()
            .filter_map(|idx| self.items.get(idx).cloned())
            .collect()
    }

    pub fn render(&mut self, stdout: &mut Stdout) -> Result<()> {
        let layout = self.layout()?;
        self.ensure_visible(&layout);

        stdout.queue(Clear(ClearType::All))?;
        self.render_header(stdout)?;
        self.render_items(stdout, &layout)?;
        self.render_footer(stdout, &layout)?;

        Ok(())
    }

    fn render_header(&self, stdout: &mut Stdout) -> Result<()> {
        stdout.queue(MoveTo(0, 0))?;
        stdout.queue(SetAttribute(Attribute::Reset))?;
        let theme = get_theme();
        stdout.queue(SetForegroundColor(theme.header_title))?;
        stdout.queue(SetAttribute(Attribute::Bold))?;
        stdout.queue(Print("Select templates  "))?;
        stdout.queue(SetAttribute(Attribute::Reset))?;
        stdout.queue(SetForegroundColor(theme.header_hint))?;
        stdout.queue(Print(
            "Space=toggle  Enter=confirm  Esc=cancel  Ctrl+A=all  Ctrl+U=clear",
        ))?;
        stdout.queue(ResetColor)?;

        stdout.queue(MoveTo(0, 1))?;
        stdout.queue(SetAttribute(Attribute::Reset))?;
        let mut filter_text = if self.search_query.is_empty() {
            String::from("Filter: showing all templates")
        } else {
            format!("Filter: {}", self.search_query)
        };
        if self.search_active {
            filter_text.push_str(" _");
        }
        stdout.queue(SetForegroundColor(theme.header_hint))?;
        stdout.queue(Print(filter_text))?;
        stdout.queue(Print("  (/ to focus, type to filter, Delete clears)"))?;
        stdout.queue(ResetColor)?;
        Ok(())
    }

    fn render_items(&self, stdout: &mut Stdout, layout: &Layout) -> Result<()> {
        if self.filtered_indices.is_empty() {
            stdout.queue(MoveTo(0, 2))?;
            let theme = get_theme();
            stdout.queue(SetForegroundColor(theme.header_hint))?;
            stdout.queue(Print("No templates match the current filter."))?;
            stdout.queue(ResetColor)?;
            return Ok(());
        }

        for row in 0..layout.rows_visible {
            for col in 0..layout.columns {
                let idx = self.viewport_offset + row * layout.columns + col;
                if idx >= self.filtered_indices.len() {
                    break;
                }

                let x = (col * layout.column_width) as u16;
                let y = (row + 2) as u16;
                stdout.queue(MoveTo(x, y))?;

                self.render_single_item(stdout, idx, layout)?;
            }
        }
        Ok(())
    }

    fn render_single_item(&self, stdout: &mut Stdout, idx: usize, layout: &Layout) -> Result<()> {
        let actual_idx = self.filtered_indices[idx];
        let is_cursor = self.cursor == idx;
        let is_selected = self.selected.contains(&actual_idx);

        if is_cursor {
            stdout.queue(SetAttribute(Attribute::Reverse))?;
        }
        let checked = if is_selected { "[x]" } else { "[ ]" };
        let theme = get_theme();
        let checkbox_color = if is_selected {
            theme.checkbox_selected
        } else {
            theme.checkbox_unselected
        };

        stdout.queue(SetForegroundColor(checkbox_color))?;
        stdout.queue(Print(checked))?;
        if is_cursor {
            // Stop reverse before the trailing space so the space is not highlighted
            stdout.queue(SetAttribute(Attribute::Reset))?;
        }
        stdout.queue(Print(" "))?;

        let name_color = if is_selected {
            theme.item_selected_text
        } else {
            theme.item_unselected_text
        };
        stdout.queue(SetForegroundColor(name_color))?;
        stdout.queue(Print(format!(
            "{:<width$}",
            &self.items[actual_idx],
            width = layout.column_width - 4
        )))?;

        stdout.queue(ResetColor)?;
        stdout.queue(SetAttribute(Attribute::Reset))?;
        Ok(())
    }

    fn render_footer(&self, stdout: &mut Stdout, layout: &Layout) -> Result<()> {
        let status = format!(
            "Selected {}/{} · Showing {}/{} · Use arrows or hjkl to move, PgUp/PgDn to scroll",
            self.selected.len(),
            self.items.len(),
            self.filtered_indices.len(),
            self.items.len()
        );
        stdout.queue(MoveTo(0, (layout.rows_visible + 3) as u16))?;
        let theme = get_theme();
        stdout.queue(SetForegroundColor(theme.footer))?;
        stdout.queue(Print(status))?;
        stdout.queue(ResetColor)?;
        Ok(())
    }
}

pub struct TerminalGuard {
    stdout: Stdout,
    active: bool,
}

impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        terminal::enable_raw_mode()?;
        stdout.execute(Hide)?;
        Ok(Self {
            stdout,
            active: true,
        })
    }

    pub fn stdout_mut(&mut self) -> &mut Stdout {
        &mut self.stdout
    }

    pub fn exit(&mut self) -> Result<()> {
        if self.active {
            self.stdout.execute(Show)?;
            execute!(self.stdout, LeaveAlternateScreen)?;
            terminal::disable_raw_mode()?;
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.exit();
    }
}
