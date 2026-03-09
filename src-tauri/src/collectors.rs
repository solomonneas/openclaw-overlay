use crate::database::Database;
use std::path::PathBuf;
use std::sync::Arc;

/// Model pricing: (input_cost_per_1m, output_cost_per_1m)
fn model_pricing(model: &str) -> (f64, f64) {
    let m = model.to_lowercase();
    if m.contains("opus") { return (15.0, 75.0); }
    if m.contains("haiku") { return (1.0, 5.0); }
    if m.contains("sonnet") { return (3.0, 15.0); }
    if m.contains("codex") || m.contains("gpt-5.3") { return (3.0, 15.0); }
    if m.contains("gpt-5.2") || m.contains("gpt-5.4") { return (3.0, 15.0); }
    if m.contains("gpt-4") { return (5.0, 15.0); }
    // Ollama / local models = free
    if m.contains("qwen") || m.contains("gemma") || m.contains("llama") || m.contains("mistral") {
        return (0.0, 0.0);
    }
    (5.0, 25.0) // default
}

fn calculate_cost(model: &str, input_tokens: i64, output_tokens: i64) -> (f64, f64) {
    let (input_rate, output_rate) = model_pricing(model);
    let input_cost = (input_tokens as f64 / 1_000_000.0) * input_rate;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * output_rate;
    (input_cost, output_cost)
}

/// Collect rate limits from ~/.openclaw/data/rate-limits.json
pub async fn collect_rate_limits(db: &Arc<Database>) {
    let path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw")
        .join("data")
        .join("rate-limits.json");

    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) => {
            log::debug!("Could not read rate-limits.json: {}", e);
            return;
        }
    };

    let data: serde_json::Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Failed to parse rate-limits.json: {}", e);
            return;
        }
    };

    // Parse providers (anthropic, codex, openai, etc.)
    if let Some(obj) = data.as_object() {
        for (provider, limits) in obj {
            if let Some(hourly) = limits.get("hourly") {
                if let Some(pct) = hourly.get("percent").and_then(|v| v.as_f64()) {
                    let reset = hourly.get("reset").and_then(|v| v.as_str()).unwrap_or("");
                    if let Err(e) = db.insert_rate_limit(provider, "hourly", pct, reset) {
                        log::warn!("Failed to insert hourly rate limit for {}: {}", provider, e);
                    }
                }
            }
            if let Some(weekly) = limits.get("weekly") {
                if let Some(pct) = weekly.get("percent").and_then(|v| v.as_f64()) {
                    let reset = weekly.get("reset").and_then(|v| v.as_str()).unwrap_or("");
                    if let Err(e) = db.insert_rate_limit(provider, "weekly", pct, reset) {
                        log::warn!("Failed to insert weekly rate limit for {}: {}", provider, e);
                    }
                }
            }
        }
    }

    log::debug!("Rate limits collected");
}

/// Collect sessions from ops-deck API
pub async fn collect_sessions(db: &Arc<Database>) {
    let url = "http://localhost:8005/api/overlay/status";

    let resp = match reqwest::get(url).await {
        Ok(r) => r,
        Err(e) => {
            log::debug!("Could not reach ops-deck API: {}", e);
            return;
        }
    };

    let data: serde_json::Value = match resp.json().await {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Failed to parse ops-deck response: {}", e);
            return;
        }
    };

    let sessions = match data.get("sessions").and_then(|s| s.as_array()) {
        Some(s) => s,
        None => {
            log::debug!("No sessions in overlay status response");
            return;
        }
    };

    let mut count = 0;
    for session in sessions {
        let id = match session.get("id").or(session.get("sessionKey")).and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };

        let label = session.get("label").and_then(|v| v.as_str());
        let model = session.get("model").and_then(|v| v.as_str()).unwrap_or("unknown");

        // Determine provider from model name
        let provider = if model.contains("claude") || model.contains("opus") || model.contains("haiku") || model.contains("sonnet") {
            "anthropic"
        } else if model.contains("gpt") || model.contains("codex") {
            "openai"
        } else if model.contains("qwen") || model.contains("gemma") || model.contains("llama") || model.contains("mistral") {
            "ollama"
        } else {
            "other"
        };

        let input_tokens = session.get("tokensUsed")
            .or(session.get("inputTokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let output_tokens = session.get("outputTokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let cache_read = session.get("cacheRead")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let cache_write = session.get("cacheWrite")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let total_tokens = input_tokens + output_tokens;
        let (input_cost, output_cost) = calculate_cost(model, input_tokens, output_tokens);
        let total_cost = input_cost + output_cost;

        if let Err(e) = db.upsert_session(
            id, label, provider, model,
            input_tokens, output_tokens, cache_read, cache_write,
            total_tokens, input_cost, output_cost, total_cost,
        ) {
            log::warn!("Failed to upsert session {}: {}", id, e);
        } else {
            count += 1;
        }
    }

    log::debug!("Collected {} sessions", count);
}
