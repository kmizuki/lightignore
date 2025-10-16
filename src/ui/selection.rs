use crate::ui::theme::get_theme;
use anyhow::Result;
use crossterm::{
    ExecutableCommand, QueueableCommand,
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::{Attribute, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::cmp::{max, min};
use std::collections::BTreeSet;
use std::io::{Stdout, Write, stdout};

pub fn select_templates(options: &[String], previous_selection: &[String]) -> Result<Vec<String>> {
    if options.is_empty() {
        return Ok(Vec::new());
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
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    break Err(anyhow::anyhow!("operation cancelled"));
                }
                KeyCode::Enter => break Ok(state.finish()),
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
            },
            Event::Resize(_, _) => state.invalidate_cache(),
            _ => {}
        }
    };

    guard.exit()?;
    result
}

pub struct SelectionState {
    items: Vec<String>,
    selected: BTreeSet<usize>,
    cursor: usize,
    viewport_offset: usize,
    cached_layout: Option<Layout>,
}

#[derive(Clone)]
struct Layout {
    columns: usize,
    column_width: usize,
    rows_visible: usize,
}

impl SelectionState {
    pub fn new(items: Vec<String>) -> Self {
        Self {
            items,
            selected: BTreeSet::new(),
            cursor: 0,
            viewport_offset: 0,
            cached_layout: None,
        }
    }

    pub fn invalidate_cache(&mut self) {
        self.cached_layout = None;
    }

    fn layout(&mut self) -> Result<Layout> {
        if let Some(layout) = &self.cached_layout {
            return Ok(layout.clone());
        }

        let (width, height) = terminal::size()?;
        let max_item_width = self.items.iter().map(|item| item.len()).max().unwrap_or(0) + 4;
        let term_width = width.saturating_sub(2) as usize;
        let mut columns = max(1, term_width / max_item_width.max(1));
        columns = min(columns, self.items.len().max(1));
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
        if self.items.is_empty() {
            self.cursor = 0;
            self.viewport_offset = 0;
            return;
        }

        if self.cursor >= self.items.len() {
            self.cursor = self.items.len() - 1;
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
        let max_offset = if self.items.len() <= viewport_capacity {
            0
        } else {
            ((self.items.len() - 1) / viewport_capacity) * viewport_capacity
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
            if self.cursor + layout.columns < self.items.len() {
                self.cursor += layout.columns;
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
        if self.cursor + 1 < self.items.len() {
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
            if self.cursor + step < self.items.len() {
                self.cursor += step;
            } else {
                self.cursor = self.items.len().saturating_sub(1);
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
        if !self.items.is_empty() {
            self.cursor = self.items.len() - 1;
            if let Ok(layout) = self.layout() {
                self.ensure_visible(&layout);
            }
        }
    }

    pub fn toggle_current(&mut self) {
        if self.selected.contains(&self.cursor) {
            self.selected.remove(&self.cursor);
        } else {
            self.selected.insert(self.cursor);
        }
    }

    pub fn select_all(&mut self) {
        self.selected.clear();
        self.selected.extend(0..self.items.len());
    }

    pub fn clear_all(&mut self) {
        self.selected.clear();
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
        Ok(())
    }

    fn render_items(&self, stdout: &mut Stdout, layout: &Layout) -> Result<()> {
        for row in 0..layout.rows_visible {
            for col in 0..layout.columns {
                let idx = self.viewport_offset + row * layout.columns + col;
                if idx >= self.items.len() {
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
        let is_cursor = self.cursor == idx;
        let is_selected = self.selected.contains(&idx);

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
            &self.items[idx],
            width = layout.column_width - 4
        )))?;

        stdout.queue(ResetColor)?;
        stdout.queue(SetAttribute(Attribute::Reset))?;
        Ok(())
    }

    fn render_footer(&self, stdout: &mut Stdout, layout: &Layout) -> Result<()> {
        let status = format!(
            "Selected {}/{} · Use arrows or hjkl to move, PgUp/PgDn to scroll",
            self.selected.len(),
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
