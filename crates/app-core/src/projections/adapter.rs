//! Presentable errors for application projection state.

use crate::module::EffectError;

pub(super) fn error(message: impl std::fmt::Display) -> EffectError {
    EffectError {
        message: message.to_string(),
    }
}
