//! Colours. Confidence uses the HTML report's palette, so the two read as
//! one product; terminals without true colour get the nearest of the 16.
use ratatui::style::{Color, Modifier, Style};
use vernier::Confidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    color: bool,
    truecolor: bool,
}

impl Theme {
    /// `color` false is `NO_COLOR`: bold and reverse video only.
    pub fn new(color: bool) -> Self {
        let truecolor = std::env::var("COLORTERM")
            .is_ok_and(|v| v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit"));
        Self { color, truecolor }
    }

    fn fg(self, rgb: (u8, u8, u8), fallback: Color) -> Style {
        if !self.color {
            return Style::new();
        }
        Style::new().fg(if self.truecolor {
            Color::Rgb(rgb.0, rgb.1, rgb.2)
        } else {
            fallback
        })
    }

    pub fn confidence(self, c: Confidence) -> Style {
        match c {
            Confidence::Observed => self.fg((0x3d, 0xdc, 0x97), Color::Green),
            Confidence::Static => self.fg((0x5a, 0xa9, 0xff), Color::Blue),
            Confidence::Inferred => self.fg((0xff, 0xb4, 0x54), Color::Yellow),
            Confidence::Uncertain => self.dim(),
        }
    }

    /// A service the change touches directly.
    pub fn changed(self) -> Style {
        self.fg((0xff, 0x5c, 0xa8), Color::Magenta)
            .add_modifier(Modifier::BOLD)
    }

    /// A partial join, an error, a line the reader should not miss.
    pub fn warn(self) -> Style {
        if self.color {
            Style::new().fg(Color::Yellow)
        } else {
            Style::new()
        }
    }

    pub fn dim(self) -> Style {
        if self.color {
            Style::new().fg(Color::DarkGray)
        } else {
            Style::new()
        }
    }

    pub fn bold(self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    pub fn selected(self) -> Style {
        Style::new().add_modifier(Modifier::REVERSED)
    }

    pub fn accent(self) -> Style {
        self.fg((0x5a, 0xa9, 0xff), Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
}
