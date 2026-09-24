//! The shared model: what a watcher reports and how it is ordered.
//!
//! These types cross a process boundary in the menu bar app, so every
//! one of them serialises; the TUI uses the same values in memory.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum FetchMsg {
    CycleStarted,
    Started(PathBuf),
    Done(PathBuf),
    CiUpdate(PathBuf, Option<CiInfo>),
    PrUpdate(PathBuf, Option<Vec<PrInfo>>),
    CycleFinished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CiState {
    Success,
    Failure,
    Running,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub age: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiInfo {
    pub state: CiState,
    pub name: String,
    pub url: String,
    pub failed_step: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortMode {
    Default,
    CiRedFirst,
    DirtyFirst,
    ByRecency,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Default => SortMode::CiRedFirst,
            SortMode::CiRedFirst => SortMode::DirtyFirst,
            SortMode::DirtyFirst => SortMode::ByRecency,
            SortMode::ByRecency => SortMode::Default,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SortMode::Default => "default",
            SortMode::CiRedFirst => "ci red first",
            SortMode::DirtyFirst => "dirty first",
            SortMode::ByRecency => "by recency",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            SortMode::Default => "default",
            SortMode::CiRedFirst => "ci_red_first",
            SortMode::DirtyFirst => "dirty_first",
            SortMode::ByRecency => "by_recency",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "ci_red_first" => SortMode::CiRedFirst,
            "dirty_first" => SortMode::DirtyFirst,
            "by_recency" => SortMode::ByRecency,
            _ => SortMode::Default,
        }
    }
}
