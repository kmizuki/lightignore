use crossterm::style::Color;
use once_cell::sync::OnceCell;

#[derive(Copy, Clone, Debug)]
pub enum ThemeKind {
    Light,
    Dark,
}

pub struct Theme {
    pub accent: Color,
    pub success: Color,
    pub checkbox_selected: Color,
    pub checkbox_unselected: Color,
    pub item_selected_text: Color,
    pub item_unselected_text: Color,
    pub footer: Color,
    pub header_title: Color,
    pub header_hint: Color,
    pub list_alt1: Color,
    pub list_alt2: Color,
}

impl Theme {
    pub fn light() -> Self {
        Self {
            accent: Color::Blue,
            success: Color::Green,
            checkbox_selected: Color::DarkGreen,
            checkbox_unselected: Color::DarkGrey,
            item_selected_text: Color::Black,
            item_unselected_text: Color::Black,
            footer: Color::Blue,
            header_title: Color::Blue,
            header_hint: Color::DarkGrey,
            list_alt1: Color::Black,
            list_alt2: Color::DarkGrey,
        }
    }

    pub fn dark() -> Self {
        Self {
            // Increase contrast in dark theme: brighter white for text, distinct accents
            accent: Color::White,
            success: Color::Green,
            checkbox_selected: Color::Green,
            checkbox_unselected: Color::DarkGrey,
            item_selected_text: Color::White,
            item_unselected_text: Color::White,
            footer: Color::White,
            header_title: Color::White,
            header_hint: Color::DarkGrey,
            list_alt1: Color::White,
            list_alt2: Color::Grey,
        }
    }
}

impl From<ThemeKind> for Theme {
    fn from(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Light => Self::light(),
            ThemeKind::Dark => Self::dark(),
        }
    }
}

static THEME: OnceCell<Theme> = OnceCell::new();

pub fn configure_theme(kind: ThemeKind) {
    let _ = THEME.set(Theme::from(kind));
}

pub fn get_theme() -> &'static Theme {
    THEME.get_or_init(Theme::light)
}

pub fn detect_theme_kind_from_env() -> ThemeKind {
    // Try to detect via COLORFGBG like "15;0" (fg;background) or "default;8"
    if let Ok(val) = std::env::var("COLORFGBG") {
        // Take last component as background
        if let Some(bg_str) = val.split(';').last() {
            if let Ok(bg) = bg_str.parse::<u8>() {
                // Common dark backgrounds are 0-7 range; 0 (black), 1-7 dark colors
                // Light backgrounds often 15 (white) or >7
                if bg >= 8 || bg == 15 {
                    return ThemeKind::Light;
                } else {
                    return ThemeKind::Dark;
                }
            }
        }
    }

    // Fallback: if NO_COLOR set, still pick based on terminal default; assume dark as typical
    ThemeKind::Dark
}
