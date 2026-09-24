//! The palette, as ratatui sees it.
//!
//! The values live in `den_core::brand`; this is only the mapping into
//! the terminal's colour type, so the TUI and the menu bar panel can
//! never drift apart.

use den_core::brand::Rgb;
use ratatui::style::Color;

const fn c(v: Rgb) -> Color {
    Color::Rgb(v.0, v.1, v.2)
}

pub const FOREST: Color = c(den_core::brand::FOREST);
pub const AMBER: Color = c(den_core::brand::AMBER);
pub const AMBER_STRONG: Color = c(den_core::brand::AMBER_STRONG);
pub const STONE: Color = c(den_core::brand::STONE);
pub const STONE_STRONG: Color = c(den_core::brand::STONE_STRONG);
pub const IVORY: Color = c(den_core::brand::IVORY);
pub const CONFLICT: Color = c(den_core::brand::CONFLICT);
