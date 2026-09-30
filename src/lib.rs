// WD-40: shared library for dev artifact scanning and cleaning.
// Used by both the macOS menu bar app and the CLI.
pub mod cache;
mod bulk;
pub mod cleaner;
pub mod config;
pub mod discover;
pub mod extent_cache;
pub mod extents;
pub mod disk;
pub mod nesting;
pub mod qos;
pub mod reclaim;
pub mod roots;
pub mod scanner;
pub mod sizes;
pub mod toolchains;
mod walk;

#[cfg(test)]
mod safety_tests;
