use anyhow::Result;
use crossterm::{
    QueueableCommand,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};
use std::cmp::max;
use std::io::{self, Write};
use std::path::PathBuf;

pub struct ColumnLayout {
    pub columns: usize,
    pub column_width: usize,
    pub rows: usize,
}

pub fn calculate_column_layout(items: &[String]) -> Result<ColumnLayout> {
    use crossterm::terminal;

    let term_width = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    let column_width = items.iter().map(|item| item.len()).max().unwrap_or(0) + 2;
    let columns = max(1, term_width / column_width.max(1));
    let rows = (items.len() + columns - 1) / columns;

    Ok(ColumnLayout {
        columns,
        column_width,
        rows,
    })
}

pub fn print_columnar_list(items: &[String], layout: &ColumnLayout) -> Result<()> {
    let mut stdout = io::stdout();

    for row in 0..layout.rows {
        for col in 0..layout.columns {
            let idx = row * layout.columns + col;
            if idx >= items.len() {
                break;
            }

            // Alternate colors for better readability
            let color = if idx % 2 == 0 {
                Color::Cyan
            } else {
                Color::Green
            };

            let item_text = format!("{:<width$}", items[idx], width = layout.column_width);
            if let Err(err) = stdout
                .queue(SetForegroundColor(color))
                .and_then(|s| s.queue(Print(item_text)))
                .and_then(|s| s.queue(ResetColor))
            {
                if err.kind() == io::ErrorKind::BrokenPipe {
                    return Ok(());
                }
                return Err(err.into());
            }
        }

        if let Err(err) = writeln!(stdout) {
            if err.kind() == io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(err.into());
        }
    }

    Ok(())
}

pub fn print_success(message: &str) -> Result<()> {
    let mut stdout = io::stdout();
    stdout.queue(SetForegroundColor(Color::Green))?;
    stdout.queue(SetAttribute(Attribute::Bold))?;
    stdout.queue(Print("✓ "))?;
    stdout.queue(SetAttribute(Attribute::Reset))?;
    stdout.queue(SetForegroundColor(Color::Green))?;
    stdout.queue(Print(message))?;
    stdout.queue(ResetColor)?;
    writeln!(stdout)?;
    Ok(())
}

pub fn print_success_message(output: &PathBuf) -> Result<()> {
    print_success(&format!("Generated {}", output.display()))
}
