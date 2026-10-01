//! Generate host payload types with the same Facet/bincode backend as Crux.

use std::{error::Error, io, path::PathBuf};

use app_core::{
    Event, ViewModel, effects::HostInfoResponse, history::QueryResponse, shell::EffectBatch,
    workspace::WorkspaceResponse,
};
use bincode as _;
use crux_core as _;
use facet as _;
use facet_generate::{
    generation::{bincode::BincodePlugin, kotlin, swift},
    reflection::RegistryBuilder,
};
use serde as _;
use thiserror as _;

fn main() -> Result<(), Box<dyn Error>> {
    let output = PathBuf::from(std::env::args_os().nth(1).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "expected an output directory")
    })?);
    // Crux 0.20's convenience feature also enables crux_macros and its deprecated
    // proc-macro-error dependency. Register explicitly to preserve our policy.
    let registry = RegistryBuilder::new()
        .add_type::<Event>()?
        .add_type::<EffectBatch>()?
        .add_type::<HostInfoResponse>()?
        .add_type::<QueryResponse>()?
        .add_type::<WorkspaceResponse>()?
        .add_type::<ViewModel>()?
        .build()?;
    swift::Installer::new("AppTypes", output.join("swift/AppTypes"))
        .plugin(BincodePlugin)
        .generate(&registry)?;
    kotlin::Installer::new("ai.idle.appcore.types", output.join("kotlin"))
        .plugin(BincodePlugin)
        .generate(&registry)?;
    Ok(())
}

use editchain_core as _;
#[cfg(not(target_arch = "wasm32"))]
use editchain_engine as _;
use idle_history as _;
use idle_protocol as _;
use serde_json as _;
#[cfg(test)]
use tempfile as _;
