use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    pub fn init() -> Result<Self, String> {
        let db_dir = Self::db_dir();
        std::fs::create_dir_all(&db_dir).map_err(|e| format!("Failed to create data dir: {}", e))?;

        let db_path = db_dir.join("usage.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| format!("Failed to set pragmas: {}", e))?;

        Self::run_migrations(&conn)?;

        log::info!("Database initialized at {:?}", db_path);
        Ok(Database { conn: Mutex::new(conn) })
    }

    fn db_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".openclaw")
            .join("data")
    }

    fn run_migrations(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                label TEXT,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                cache_read_tokens INTEGER DEFAULT 0,
                cache_write_tokens INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                input_cost REAL DEFAULT 0,
                output_cost REAL DEFAULT 0,
                total_cost REAL DEFAULT 0,
                started_at TEXT,
                last_active_at TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS rate_limit_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                limit_type TEXT NOT NULL,
                used_pct REAL,
                resets_at TEXT,
                captured_at TEXT DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_rls_provider_time
                ON rate_limit_snapshots(provider, captured_at);

            CREATE TABLE IF NOT EXISTS daily_rollups (
                date TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                total_tokens INTEGER DEFAULT 0,
                total_cost REAL DEFAULT 0,
                session_count INTEGER DEFAULT 0,
                PRIMARY KEY (date, provider, model)
            );

            CREATE TABLE IF NOT EXISTS subscriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                monthly_cost REAL NOT NULL,
                provider TEXT,
                enabled INTEGER DEFAULT 1,
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS ollama_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                model TEXT NOT NULL,
                prompt_tokens INTEGER DEFAULT 0,
                completion_tokens INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                duration_ms INTEGER DEFAULT 0,
                recorded_at TEXT DEFAULT (datetime('now'))
            );
            "
        ).map_err(|e| format!("Migration failed: {}", e))?;

        Ok(())
    }

    // ── Rate Limit Snapshots ──

    pub fn insert_rate_limit(&self, provider: &str, limit_type: &str, used_pct: f64, resets_at: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO rate_limit_snapshots (provider, limit_type, used_pct, resets_at) VALUES (?1, ?2, ?3, ?4)",
            params![provider, limit_type, used_pct, resets_at],
        ).map_err(|e| format!("Insert rate limit failed: {}", e))?;
        Ok(())
    }

    pub fn get_rate_limit_history(&self, provider: Option<&str>, hours: u32) -> Result<Vec<serde_json::Value>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let hours_str = format!("-{} hours", hours);
        let (query, has_provider) = if provider.is_some() {
            (
                "SELECT provider, limit_type, used_pct, resets_at, captured_at
                 FROM rate_limit_snapshots
                 WHERE provider = ?1 AND captured_at >= datetime('now', ?2)
                 ORDER BY captured_at DESC".to_string(),
                true,
            )
        } else {
            (
                "SELECT provider, limit_type, used_pct, resets_at, captured_at
                 FROM rate_limit_snapshots
                 WHERE captured_at >= datetime('now', ?1)
                 ORDER BY captured_at DESC".to_string(),
                false,
            )
        };

        let mut stmt = conn.prepare(&query).map_err(|e| e.to_string())?;
        let rows = if has_provider {
            stmt.query_map(params![provider.unwrap(), hours_str], |row| {
                Ok(serde_json::json!({
                    "provider": row.get::<_, String>(0)?,
                    "limitType": row.get::<_, String>(1)?,
                    "usedPct": row.get::<_, f64>(2)?,
                    "resetsAt": row.get::<_, String>(3)?,
                    "capturedAt": row.get::<_, String>(4)?,
                }))
            }).map_err(|e| e.to_string())?
        } else {
            stmt.query_map(params![hours_str], |row| {
                Ok(serde_json::json!({
                    "provider": row.get::<_, String>(0)?,
                    "limitType": row.get::<_, String>(1)?,
                    "usedPct": row.get::<_, f64>(2)?,
                    "resetsAt": row.get::<_, String>(3)?,
                    "capturedAt": row.get::<_, String>(4)?,
                }))
            }).map_err(|e| e.to_string())?
        };

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| e.to_string())?);
        }
        Ok(results)
    }

    // ── Sessions ──

    pub fn upsert_session(&self, id: &str, label: Option<&str>, provider: &str, model: &str,
                          input_tokens: i64, output_tokens: i64, cache_read: i64, cache_write: i64,
                          total_tokens: i64, input_cost: f64, output_cost: f64, total_cost: f64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO sessions (id, label, provider, model, input_tokens, output_tokens,
             cache_read_tokens, cache_write_tokens, total_tokens, input_cost, output_cost, total_cost, last_active_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
             label=COALESCE(?2, label), input_tokens=?5, output_tokens=?6,
             cache_read_tokens=?7, cache_write_tokens=?8, total_tokens=?9,
             input_cost=?10, output_cost=?11, total_cost=?12, last_active_at=datetime('now')",
            params![id, label, provider, model, input_tokens, output_tokens,
                    cache_read, cache_write, total_tokens, input_cost, output_cost, total_cost],
        ).map_err(|e| format!("Upsert session failed: {}", e))?;
        Ok(())
    }

    pub fn get_sessions(&self, days: u32, sort_by: &str, limit: u32) -> Result<Vec<serde_json::Value>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let order = match sort_by {
            "tokens" => "total_tokens DESC",
            "date" => "last_active_at DESC",
            _ => "total_cost DESC",
        };
        let query = format!(
            "SELECT id, label, provider, model, input_tokens, output_tokens,
             cache_read_tokens, cache_write_tokens, total_tokens,
             input_cost, output_cost, total_cost, started_at, last_active_at, created_at
             FROM sessions
             WHERE created_at >= datetime('now', '-{} days')
             ORDER BY {} LIMIT {}",
            days, order, limit
        );
        let mut stmt = conn.prepare(&query).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "label": row.get::<_, Option<String>>(1)?,
                "provider": row.get::<_, String>(2)?,
                "model": row.get::<_, String>(3)?,
                "inputTokens": row.get::<_, i64>(4)?,
                "outputTokens": row.get::<_, i64>(5)?,
                "cacheReadTokens": row.get::<_, i64>(6)?,
                "cacheWriteTokens": row.get::<_, i64>(7)?,
                "totalTokens": row.get::<_, i64>(8)?,
                "inputCost": row.get::<_, f64>(9)?,
                "outputCost": row.get::<_, f64>(10)?,
                "totalCost": row.get::<_, f64>(11)?,
                "startedAt": row.get::<_, Option<String>>(12)?,
                "lastActiveAt": row.get::<_, Option<String>>(13)?,
                "createdAt": row.get::<_, String>(14)?,
            }))
        }).map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| e.to_string())?);
        }
        Ok(results)
    }

    pub fn get_dashboard_summary(&self, days: u32) -> Result<serde_json::Value, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        // Totals for period
        let mut stmt = conn.prepare(&format!(
            "SELECT COALESCE(SUM(total_tokens),0), COALESCE(SUM(total_cost),0), COUNT(*),
             COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0)
             FROM sessions WHERE created_at >= datetime('now', '-{} days')", days
        )).map_err(|e| e.to_string())?;

        let (total_tokens, total_cost, total_sessions, cache_read, cache_write): (i64, f64, i64, i64, i64) =
            stmt.query_row([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            }).map_err(|e| e.to_string())?;

        let cache_hit_rate = if (cache_read + cache_write) > 0 {
            (cache_read as f64 / (cache_read + cache_write) as f64) * 100.0
        } else { 0.0 };

        // Active models
        let mut stmt = conn.prepare(&format!(
            "SELECT DISTINCT model FROM sessions WHERE created_at >= datetime('now', '-{} days')", days
        )).map_err(|e| e.to_string())?;
        let models: Vec<String> = stmt.query_map([], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        // This week vs last week
        let mut stmt = conn.prepare(
            "SELECT COALESCE(SUM(total_cost),0), COALESCE(SUM(total_tokens),0), COUNT(*)
             FROM sessions WHERE created_at >= datetime('now', '-7 days')"
        ).map_err(|e| e.to_string())?;
        let (this_week_cost, this_week_tokens, this_week_sessions): (f64, i64, i64) =
            stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| e.to_string())?;

        let mut stmt = conn.prepare(
            "SELECT COALESCE(SUM(total_cost),0), COALESCE(SUM(total_tokens),0), COUNT(*)
             FROM sessions WHERE created_at >= datetime('now', '-14 days') AND created_at < datetime('now', '-7 days')"
        ).map_err(|e| e.to_string())?;
        let (last_week_cost, last_week_tokens, last_week_sessions): (f64, i64, i64) =
            stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| e.to_string())?;

        let cost_change = if last_week_cost > 0.0 { ((this_week_cost - last_week_cost) / last_week_cost) * 100.0 } else { 0.0 };
        let token_change = if last_week_tokens > 0 { ((this_week_tokens - last_week_tokens) as f64 / last_week_tokens as f64) * 100.0 } else { 0.0 };
        let session_change = if last_week_sessions > 0 { ((this_week_sessions - last_week_sessions) as f64 / last_week_sessions as f64) * 100.0 } else { 0.0 };

        // Model stats
        let mut stmt = conn.prepare(&format!(
            "SELECT model, provider, SUM(total_tokens), SUM(total_cost), COUNT(*)
             FROM sessions WHERE created_at >= datetime('now', '-{} days')
             GROUP BY model ORDER BY SUM(total_cost) DESC", days
        )).map_err(|e| e.to_string())?;
        let model_stats: Vec<serde_json::Value> = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "model": row.get::<_, String>(0)?,
                "provider": row.get::<_, String>(1)?,
                "tokens": row.get::<_, i64>(2)?,
                "cost": row.get::<_, f64>(3)?,
                "sessions": row.get::<_, i64>(4)?,
            }))
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();

        // Provider stats
        let mut stmt = conn.prepare(&format!(
            "SELECT provider, SUM(total_tokens), SUM(total_cost), COUNT(*), COUNT(DISTINCT model)
             FROM sessions WHERE created_at >= datetime('now', '-{} days')
             GROUP BY provider ORDER BY SUM(total_cost) DESC", days
        )).map_err(|e| e.to_string())?;
        let provider_stats: Vec<serde_json::Value> = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "provider": row.get::<_, String>(0)?,
                "tokens": row.get::<_, i64>(1)?,
                "cost": row.get::<_, f64>(2)?,
                "sessions": row.get::<_, i64>(3)?,
                "modelCount": row.get::<_, i64>(4)?,
            }))
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();

        // Daily data
        let mut stmt = conn.prepare(&format!(
            "SELECT date(created_at) as d, SUM(total_tokens), SUM(total_cost), COUNT(*)
             FROM sessions WHERE created_at >= datetime('now', '-{} days')
             GROUP BY d ORDER BY d", days
        )).map_err(|e| e.to_string())?;
        let daily_data: Vec<serde_json::Value> = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "tokens": row.get::<_, i64>(1)?,
                "cost": row.get::<_, f64>(2)?,
                "sessions": row.get::<_, i64>(3)?,
            }))
        }).map_err(|e| e.to_string())?
        .filter_map(|r| r.ok()).collect();

        Ok(serde_json::json!({
            "summary": {
                "totalTokens": total_tokens,
                "totalCost": total_cost,
                "totalSessions": total_sessions,
                "cacheHitRate": cache_hit_rate,
                "cacheRead": cache_read,
                "cacheWrite": cache_write,
                "activeModels": models,
                "weekOverWeek": {
                    "costChange": cost_change,
                    "tokenChange": token_change,
                    "sessionChange": session_change,
                }
            },
            "dailyData": daily_data,
            "modelStats": model_stats,
            "providerStats": provider_stats,
        }))
    }

    // ── Subscriptions ──

    pub fn list_subscriptions(&self) -> Result<Vec<serde_json::Value>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT id, name, monthly_cost, provider, enabled, created_at FROM subscriptions ORDER BY id"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "name": row.get::<_, String>(1)?,
                "monthlyCost": row.get::<_, f64>(2)?,
                "provider": row.get::<_, Option<String>>(3)?,
                "enabled": row.get::<_, bool>(4)?,
                "createdAt": row.get::<_, String>(5)?,
            }))
        }).map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| e.to_string())?);
        }
        Ok(results)
    }

    pub fn add_subscription(&self, name: &str, cost: f64, provider: Option<&str>) -> Result<serde_json::Value, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO subscriptions (name, monthly_cost, provider) VALUES (?1, ?2, ?3)",
            params![name, cost, provider],
        ).map_err(|e| e.to_string())?;
        let id = conn.last_insert_rowid();
        Ok(serde_json::json!({"id": id, "name": name, "monthlyCost": cost, "provider": provider, "enabled": true}))
    }

    pub fn update_subscription(&self, id: i64, name: Option<&str>, cost: Option<f64>, enabled: Option<bool>) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        if let Some(n) = name {
            conn.execute("UPDATE subscriptions SET name = ?1 WHERE id = ?2", params![n, id])
                .map_err(|e| e.to_string())?;
        }
        if let Some(c) = cost {
            conn.execute("UPDATE subscriptions SET monthly_cost = ?1 WHERE id = ?2", params![c, id])
                .map_err(|e| e.to_string())?;
        }
        if let Some(e) = enabled {
            conn.execute("UPDATE subscriptions SET enabled = ?1 WHERE id = ?2", params![e, id])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete_subscription(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM subscriptions WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
