#![forbid(unsafe_code)]

pub mod adapters;
pub mod cli;
pub mod config;
pub mod discovery;
pub mod ingest;
pub mod models;
pub mod normalize;
pub mod snapshot;
pub mod sqlite;
pub mod utils;
pub mod validate;

pub use cli::app::{Cli, Command};
