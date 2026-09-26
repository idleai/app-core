//! Export the JSON host contract for clients and schema drift verification.

use std::{error::Error, fs, io};

use app_core::shell::ShellContract;
use crux_core as _;
use serde as _;
use thiserror as _;

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os().nth(1).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "expected a schema output path")
    })?;
    let schema = schemars::schema_for!(ShellContract);
    fs::write(path, serde_json::to_string_pretty(&schema)?)?;
    Ok(())
}
