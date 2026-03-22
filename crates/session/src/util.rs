use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn epoch_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

pub(crate) fn sanitize_key(key: &str) -> String {
    key.replace(':', "_")
}
