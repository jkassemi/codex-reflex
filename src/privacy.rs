use serde_json::{Map, Value};

const REDACTED: &str = "[REDACTED]";

pub fn redact_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_text(text)),
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        Value::Object(map) => {
            let mut redacted = Map::new();
            for (key, value) in map {
                if secret_key_name(key) {
                    redacted.insert(key.clone(), Value::String(REDACTED.to_string()));
                } else {
                    redacted.insert(key.clone(), redact_value(value));
                }
            }
            Value::Object(redacted)
        }
        other => other.clone(),
    }
}

pub fn redact_text(input: &str) -> String {
    let mut output = redact_authorization_headers(input);
    output = redact_inline_bearer_tokens(&output);
    output = redact_secret_headers(&output);
    output = redact_inline_secret_header_values(&output);
    output = redact_url_secret_params(&output);
    output = redact_assignments(&output);
    output = redact_private_keys(&output);
    output = redact_github_tokens(&output);
    output
}

pub fn contains_raw_secret(input: &str) -> bool {
    input.contains("AWS_SECRET_ACCESS_KEY=")
        || input.contains("Authorization: Bearer abc123")
        || input.contains("ghp_")
        || input.contains("-----BEGIN PRIVATE KEY-----")
}

fn secret_key_name(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("authorization")
        || lower.contains("cookie")
        || lower.contains("password")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("private_key")
        || lower.contains("session_id")
}

fn redact_authorization_headers(input: &str) -> String {
    let mut lines = Vec::new();
    for line in input.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.trim_start().starts_with("authorization:")
            || lower.trim_start().starts_with("cookie:")
        {
            let prefix = line.split(':').next().unwrap_or("Authorization");
            lines.push(format!("{prefix}: {REDACTED}"));
        } else {
            lines.push(line.to_string());
        }
    }
    if input.ends_with('\n') {
        lines.push(String::new());
    }
    lines.join("\n")
}

fn redact_assignments(input: &str) -> String {
    input
        .split_whitespace()
        .map(|token| {
            let trimmed = token.trim_matches(|c| c == '\'' || c == '"');
            if let Some((key, _value)) = trimmed.split_once('=') {
                if secret_key_name(key) || secret_assignment_name(key) {
                    let prefix = token.find(key).map(|idx| &token[..idx]).unwrap_or_default();
                    let suffix = token
                        .chars()
                        .last()
                        .filter(|c| *c == '\'' || *c == '"')
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    format!("{prefix}{key}={REDACTED}{suffix}")
                } else {
                    token.to_string()
                }
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_inline_bearer_tokens(input: &str) -> String {
    let mut remaining = input;
    let mut output = String::new();
    while let Some(index) = remaining.to_ascii_lowercase().find("bearer ") {
        let (before, after_before) = remaining.split_at(index);
        let marker = &after_before[..7];
        output.push_str(before);
        output.push_str(marker);
        output.push_str(REDACTED);
        let after_marker = &after_before[7..];
        let end = after_marker
            .find(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == ';')
            .unwrap_or(after_marker.len());
        remaining = &after_marker[end..];
    }
    output.push_str(remaining);
    output
}

fn redact_secret_headers(input: &str) -> String {
    let mut lines = Vec::new();
    for line in input.lines() {
        let lower = line.to_ascii_lowercase();
        let trimmed = lower.trim_start();
        if trimmed.starts_with("x-api-key:")
            || trimmed.starts_with("x-auth-token:")
            || trimmed.starts_with("proxy-authorization:")
        {
            let prefix = line.split(':').next().unwrap_or("secret");
            lines.push(format!("{prefix}: {REDACTED}"));
        } else {
            lines.push(line.to_string());
        }
    }
    if input.ends_with('\n') {
        lines.push(String::new());
    }
    lines.join("\n")
}

fn redact_inline_secret_header_values(input: &str) -> String {
    let mut output = input.to_string();
    for marker in [
        "x-api-key:",
        "x-auth-token:",
        "proxy-authorization:",
        "authorization:",
        "cookie:",
    ] {
        output = redact_after_inline_marker(&output, marker);
    }
    output
}

fn redact_after_inline_marker(input: &str, marker: &str) -> String {
    let mut remaining = input;
    let mut output = String::new();
    while let Some(index) = remaining.to_ascii_lowercase().find(marker) {
        let (before, after_before) = remaining.split_at(index);
        let actual_marker = &after_before[..marker.len()];
        output.push_str(before);
        output.push_str(actual_marker);
        output.push(' ');
        output.push_str(REDACTED);
        let after_marker = after_before[marker.len()..].trim_start();
        let end = after_marker
            .find(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == ';')
            .unwrap_or(after_marker.len());
        remaining = &after_marker[end..];
    }
    output.push_str(remaining);
    output
}

fn redact_url_secret_params(input: &str) -> String {
    let secret_params = [
        "access_token=",
        "id_token=",
        "refresh_token=",
        "client_secret=",
        "api_key=",
        "apikey=",
        "code=",
        "sig=",
        "signature=",
    ];
    let mut output = input.to_string();
    for param in secret_params {
        output = redact_param_value(&output, param);
    }
    output
}

fn redact_param_value(input: &str, param: &str) -> String {
    let mut remaining = input;
    let mut output = String::new();
    while let Some(index) = remaining.to_ascii_lowercase().find(param) {
        let (before, after_before) = remaining.split_at(index);
        let key = &after_before[..param.len()];
        output.push_str(before);
        output.push_str(key);
        output.push_str(REDACTED);
        let after_value_start = &after_before[param.len()..];
        let end = after_value_start
            .find(|c: char| c == '&' || c.is_whitespace() || c == '\'' || c == '"')
            .unwrap_or(after_value_start.len());
        remaining = &after_value_start[end..];
    }
    output.push_str(remaining);
    output
}

fn secret_assignment_name(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower == "aws_access_key_id"
        || lower == "aws_session_token"
        || lower == "github_token"
        || lower == "openai_api_key"
        || lower == "anthropic_api_key"
}

fn redact_private_keys(input: &str) -> String {
    let begin = "-----BEGIN ";
    let end = "-----END ";
    if input.contains(begin) && input.contains(end) {
        return format!("{REDACTED} private key material");
    }
    input.to_string()
}

fn redact_github_tokens(input: &str) -> String {
    input
        .split_whitespace()
        .map(|token| {
            if token.starts_with("ghp_") || token.starts_with("github_pat_") {
                REDACTED.to_string()
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_known_secret_shapes() {
        let input = "Authorization: Bearer abc123\nAWS_SECRET_ACCESS_KEY=raw ghp_secret";
        let redacted = redact_text(input);
        assert!(!redacted.contains("Bearer abc123"));
        assert!(!redacted.contains("raw"));
        assert!(!redacted.contains("ghp_secret"));
        assert!(redacted.contains(REDACTED));
    }

    #[test]
    fn redacts_secret_object_keys() {
        let value = json!({"password": "abc", "nested": {"token": "def"}, "ok": "pytest"});
        let redacted = redact_value(&value);
        let text = redacted.to_string();
        assert!(!text.contains("abc"));
        assert!(!text.contains("def"));
        assert!(text.contains("pytest"));
    }

    #[test]
    fn redacts_additional_header_url_and_case_variants() {
        let input = "x-api-key: raw\ncurl 'https://x.test?a=1&access_token=tok&code=once' -H 'authorization: bearer lower'";
        let redacted = redact_text(input);
        assert!(!redacted.contains("raw"));
        assert!(!redacted.contains("tok&"));
        assert!(!redacted.contains("once'"));
        assert!(!redacted.contains("bearer lower"));
    }
}
