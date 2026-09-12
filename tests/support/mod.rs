//! Shared helpers for integration tests.
//!
//! Each integration test binary imports only the pieces it needs; suppress
//! dead-code warnings because the whole module is compiled for every test.

#![allow(dead_code)]

pub mod command;
pub mod markdown;
pub mod tempdir;

pub const BIN: &str = env!("CARGO_BIN_EXE_tmux-agent-status");
