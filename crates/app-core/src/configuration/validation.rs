//! Document shape and response binding checks; policy semantics remain external.

use super::{
    ConfigurationContext, ConfigurationError, ConfigurationErrorKind, ConfigurationOperation,
    ConfigurationRecord, ConfigurationSnapshot, ConfigurationValue, DOCUMENT_VERSION,
};

pub(super) fn error(kind: ConfigurationErrorKind, message: &str) -> ConfigurationError {
    ConfigurationError {
        kind,
        message: message.into(),
    }
}

pub(super) fn invalid(message: &str) -> ConfigurationError {
    error(ConfigurationErrorKind::InvalidData, message)
}

pub(super) fn context(value: &ConfigurationContext) -> Result<(), ConfigurationError> {
    if [
        &value.provider,
        &value.workspace_id,
        &value.contributor_id,
        &value.chain,
    ]
    .iter()
    .any(|value| value.trim().is_empty())
    {
        return Err(error(
            ConfigurationErrorKind::InvalidInput,
            "Configuration context requires a provider, workspace, contributor and chain",
        ));
    }
    Ok(())
}

pub(super) fn value(value: &ConfigurationValue) -> Result<(), ConfigurationError> {
    if value.json.len() > super::MAX_DOCUMENT_BYTES {
        return Err(error(
            ConfigurationErrorKind::InvalidInput,
            "Configuration exceeds the 256 KiB UTF-8 limit",
        ));
    }
    if value.schema_version != DOCUMENT_VERSION {
        return Err(error(
            ConfigurationErrorKind::Unsupported,
            "This document version is read-only in this client",
        ));
    }
    match serde_json::from_str::<serde_json::Value>(&value.json) {
        Ok(serde_json::Value::Object(_)) => Ok(()),
        Ok(_) => Err(error(
            ConfigurationErrorKind::InvalidInput,
            "Configuration must be a JSON object",
        )),
        Err(problem) => Err(error(
            ConfigurationErrorKind::InvalidInput,
            &format!("Invalid JSON: {problem}"),
        )),
    }
}

pub(super) fn snapshot(
    value: &ConfigurationSnapshot,
    operation: &ConfigurationOperation,
) -> Result<(), ConfigurationError> {
    if value.context != operation.context || value.document != operation.document {
        return Err(invalid(
            "Configuration response does not match its request scope",
        ));
    }
    if let Some(record) = &value.record {
        if record.revision == 0 || record.value.schema_version == 0 {
            return Err(invalid(
                "Configuration revision and document version must be positive",
            ));
        }
        // Retain unknown formats verbatim, but never permit this client to edit them.
        if record.value.schema_version == DOCUMENT_VERSION {
            self::value(&record.value).map_err(|problem| invalid(&problem.message))?;
        }
    }
    Ok(())
}

pub(super) fn progression(
    current: Option<&ConfigurationRecord>,
    next: Option<&ConfigurationRecord>,
) -> Result<(), ConfigurationError> {
    if let Some(current) = current
        && next.is_none_or(|next| {
            next.revision < current.revision
                || (next.revision == current.revision && next != current)
        })
    {
        return Err(invalid(
            "Configuration response regresses or changes an existing revision",
        ));
    }
    Ok(())
}
