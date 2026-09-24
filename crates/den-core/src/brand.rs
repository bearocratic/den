//! The brand palette, once, as plain sRGB.
//!
//! The TUI turns these into ratatui colours and the menu bar panel
//! into CSS, so neither surface keeps its own copy of the values.

/// An sRGB triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb`, for the panel's stylesheet.
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
}

pub const FOREST: Rgb = Rgb(0x5B, 0xA3, 0x7C);
pub const AMBER: Rgb = Rgb(0xD4, 0x9A, 0x4F);
pub const AMBER_STRONG: Rgb = Rgb(0xE4, 0xB6, 0x6E);
pub const STONE: Rgb = Rgb(0x9E, 0x9B, 0x96);
pub const STONE_STRONG: Rgb = Rgb(0x76, 0x74, 0x70);
pub const IVORY: Rgb = Rgb(0xED, 0xE9, 0xE2);
pub const CONFLICT: Rgb = Rgb(0xC2, 0x4A, 0x3F);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_six_digits_with_a_hash() {
        assert_eq!(FOREST.hex(), "#5BA37C");
        assert_eq!(CONFLICT.hex(), "#C24A3F");
    }
}
