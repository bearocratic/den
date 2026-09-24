//! What den knows, minus how it is drawn.
//!
//! The TUI and the macOS menu bar app are two surfaces over the same
//! watcher: this crate holds the repository scan, the session store,
//! the GitHub lookups, the ordering rule and the palette, and nothing
//! that assumes a terminal or a window.

pub mod brand;
pub mod github;
pub mod model;
pub mod order;
pub mod repo;
pub mod session;

pub use model::{CiInfo, CiState, FetchMsg, PrInfo, SortMode};
pub use order::{display_order, state_priority, OrderView};
pub use repo::{is_noise, CommitInfo, RepoStatus, TagInfo};
