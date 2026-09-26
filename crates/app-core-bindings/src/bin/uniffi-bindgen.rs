//! Generate native host bindings from the compiled library's `UniFFI` metadata.

use app_core as _;
use app_core_bindings as _;
use thiserror as _;

fn main() {
    uniffi::uniffi_bindgen_main();
}
