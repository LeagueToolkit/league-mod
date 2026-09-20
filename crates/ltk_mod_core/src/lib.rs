//! Core shared logic for LeagueToolkit mod management.
//!
//! This crate provides common functionality used by both the `league-mod` CLI
//! and the `ltk-manager` GUI application.

mod league_path;

pub use league_path::{auto_detect_league_path, is_valid_league_path};
