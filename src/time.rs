use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn now_text() -> String {
    format!("unix_ms:{}", now_millis())
}

pub fn future_text(hours: u64) -> String {
    let millis = now_millis() + u128::from(hours) * 60 * 60 * 1000;
    format!("unix_ms:{millis}")
}
