use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::Connection;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn with_conn<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        let c = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        f(&c)
    }

    pub fn insert_event(&self, ev: &EventRow) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO events (source_id, timestamp, event_type, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_usd, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    ev.source_id,
                    ev.timestamp,
                    ev.event_type,
                    ev.model,
                    ev.input_tokens,
                    ev.output_tokens,
                    ev.cache_read_tokens,
                    ev.cache_write_tokens,
                    ev.cost_usd,
                    ev.metadata,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_tailer_offset(&self, path: &str) -> Result<u64> {
        self.with_conn(|c| {
            let mut stmt =
                c.prepare("SELECT byte_offset FROM tailer_offsets WHERE file_path = ?1")?;
            let mut rows = stmt.query([path])?;
            if let Some(row) = rows.next()? {
                let off: i64 = row.get(0)?;
                Ok(off as u64)
            } else {
                Ok(0)
            }
        })
    }

    pub fn set_tailer_offset(&self, path: &str, offset: u64) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO tailer_offsets (file_path, byte_offset, last_seen_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(file_path) DO UPDATE SET byte_offset = excluded.byte_offset, last_seen_at = excluded.last_seen_at",
                rusqlite::params![path, offset as i64, chrono::Utc::now().timestamp_millis()],
            )?;
            Ok(())
        })
    }

    /// Aggregate events older than `now_ms` into hourly_rollups, skipping the
    /// currently-in-progress hour. Idempotent: uses INSERT OR REPLACE keyed by
    /// (hour_start, source_id, profile_id, model). Safe to call repeatedly.
    pub fn rollup_hours(&self, now_ms: i64) -> Result<usize> {
        self.with_conn(|c| {
            // Round now to the hour; only aggregate fully-elapsed hours.
            let hour_ms = 3_600_000i64;
            let cur_hour = (now_ms / hour_ms) * hour_ms;
            let mut stmt = c.prepare(
                "SELECT (timestamp / 3600000) * 3600000 AS hour_start,
                        source_id, COALESCE(model, '') AS model,
                        COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cache_read_tokens), 0),
                        COALESCE(SUM(cache_write_tokens), 0),
                        COALESCE(SUM(cost_usd), 0.0)
                 FROM events
                 WHERE timestamp < ?1
                 GROUP BY hour_start, source_id, model",
            )?;
            let rows: Vec<(i64, String, String, i64, i64, i64, i64, f64)> = stmt
                .query_map([cur_hour], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let n = rows.len();
            for (hour, sid, model, i, o, cr, cw, cost) in rows {
                c.execute(
                    "INSERT OR REPLACE INTO hourly_rollups
                     (hour_start, source_id, profile_id, model,
                      input_tokens, output_tokens, cache_read_tokens,
                      cache_write_tokens, cost_usd)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    rusqlite::params![hour, sid, "you", model, i, o, cr, cw, cost],
                )?;
            }
            Ok(n)
        })
    }

    /// Return (bucket_start_ms, tokens_in, tokens_out) bucketed to
    /// `bucket_ms`, for the last `bucket_count` buckets ending at `now_ms`.
    /// Pulls directly from events; used to seed spectrum rings at startup.
    pub fn bucket_tokens(
        &self,
        now_ms: i64,
        bucket_ms: i64,
        bucket_count: i64,
    ) -> Result<Vec<(i64, i64, i64)>> {
        self.with_conn(|c| {
            let since = now_ms - bucket_ms * bucket_count;
            let mut stmt = c.prepare(
                "SELECT (timestamp / ?1) * ?1 AS b,
                        COALESCE(SUM(input_tokens + cache_read_tokens + cache_write_tokens), 0),
                        COALESCE(SUM(output_tokens), 0)
                 FROM events
                 WHERE timestamp >= ?2
                 GROUP BY b
                 ORDER BY b ASC",
            )?;
            let rows: Vec<(i64, i64, i64)> = stmt
                .query_map(rusqlite::params![bucket_ms, since], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn sum_cost_since(&self, since_ms: i64) -> Result<f64> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT COALESCE(SUM(cost_usd), 0.0) FROM events WHERE timestamp >= ?1",
            )?;
            let v: f64 = stmt.query_row([since_ms], |r| r.get(0))?;
            Ok(v)
        })
    }

    pub fn sum_tokens_since(&self, since_ms: i64) -> Result<(i64, i64)> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0) FROM events WHERE timestamp >= ?1"
            )?;
            let (i, o): (i64, i64) = stmt.query_row([since_ms], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok((i, o))
        })
    }
}

#[derive(Debug, Clone)]
pub struct EventRow {
    pub source_id: String,
    pub timestamp: i64,
    pub event_type: String,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
    pub metadata: Option<String>,
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            model TEXT,
            input_tokens INTEGER,
            output_tokens INTEGER,
            cache_read_tokens INTEGER,
            cache_write_tokens INTEGER,
            cost_usd REAL,
            metadata TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
        CREATE INDEX IF NOT EXISTS idx_events_source ON events(source_id);

        CREATE TABLE IF NOT EXISTS hourly_rollups (
            hour_start INTEGER NOT NULL,
            source_id TEXT NOT NULL,
            profile_id TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cache_read_tokens INTEGER NOT NULL,
            cache_write_tokens INTEGER NOT NULL,
            cost_usd REAL NOT NULL,
            PRIMARY KEY (hour_start, source_id, profile_id, model)
        );

        CREATE TABLE IF NOT EXISTS alerts (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            source_id TEXT,
            timestamp INTEGER NOT NULL,
            value REAL,
            threshold REAL,
            resolved INTEGER NOT NULL DEFAULT 0,
            resolved_at INTEGER,
            metadata TEXT
        );

        CREATE TABLE IF NOT EXISTS tailer_offsets (
            file_path TEXT PRIMARY KEY,
            byte_offset INTEGER NOT NULL,
            last_seen_at INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}
