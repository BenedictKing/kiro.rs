//! 请求日志 SQLite 存储模块
//!
//! 使用 WAL 模式的 SQLite 持久化请求日志，支持按时间/状态/凭据筛选和分页查询。
//! 写入通过 mpsc channel 异步化，避免在请求热路径上同步 IO。

use rusqlite::{Connection, params};
use serde::Serialize;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::kiro::trace::{AttemptOutcome, TraceRecord};

/// 默认日志保留天数
const DEFAULT_RETENTION_DAYS: u32 = 7;

/// 后台写入 channel 容量
const CHANNEL_CAPACITY: usize = 4096;

/// 请求日志摘要（用于列表查询）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogSummary {
    pub id: i64,
    pub ts: String,
    pub ts_epoch: i64,
    pub path: String,
    pub model: Option<String>,
    pub is_stream: bool,
    pub final_status: i32,
    pub final_credential_id: u64,
    pub duration_ms: i64,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub total_attempts: i32,
    pub error: Option<String>,
    pub attempts: Vec<AttemptSummary>,
}

/// 重试尝试摘要
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptSummary {
    pub try_number: i32,
    pub credential_id: u64,
    pub status_code: i32,
    pub outcome: String,
    pub duration_ms: i64,
    pub error: Option<String>,
}

/// Trace 数据库句柄
pub struct TraceDb {
    /// 写入 channel 发送端
    tx: mpsc::Sender<TraceRecord>,
    /// 数据库文件路径（用于只读查询）
    db_path: PathBuf,
    /// 保留天数
    retention_days: u32,
}

impl TraceDb {
    /// 初始化数据库并启动后台写入任务
    pub fn new(db_path: PathBuf) -> anyhow::Result<Self> {
        Self::with_retention(db_path, DEFAULT_RETENTION_DAYS)
    }

    /// 使用自定义保留天数初始化
    pub fn with_retention(db_path: PathBuf, retention_days: u32) -> anyhow::Result<Self> {
        Self::init_schema(&db_path)?;

        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let write_path = db_path.clone();

        tokio::spawn(async move {
            Self::write_loop(write_path, rx).await;
        });

        Ok(Self {
            tx,
            db_path,
            retention_days,
        })
    }

    /// 初始化数据库 schema
    fn init_schema(db_path: &PathBuf) -> anyhow::Result<()> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS request_logs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 ts TEXT NOT NULL,
                 ts_epoch INTEGER NOT NULL,
                 path TEXT NOT NULL,
                 model TEXT,
                 is_stream INTEGER,
                 final_status INTEGER,
                 final_credential_id INTEGER,
                 duration_ms INTEGER,
                 input_tokens INTEGER,
                 output_tokens INTEGER,
                 total_attempts INTEGER,
                 error TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_logs_ts ON request_logs(ts_epoch);
             CREATE INDEX IF NOT EXISTS idx_logs_status ON request_logs(final_status);

             CREATE TABLE IF NOT EXISTS request_attempts (
                 log_id INTEGER NOT NULL,
                 try_number INTEGER NOT NULL,
                 credential_id INTEGER,
                 status_code INTEGER,
                 outcome TEXT,
                 duration_ms INTEGER,
                 error TEXT,
                 PRIMARY KEY (log_id, try_number),
                 FOREIGN KEY (log_id) REFERENCES request_logs(id) ON DELETE CASCADE
             );",
        )?;
        Ok(())
    }

    /// 后台写入循环
    async fn write_loop(db_path: PathBuf, mut rx: mpsc::Receiver<TraceRecord>) {
        let conn = match Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("打开 Trace 数据库失败: {}", e);
                return;
            }
        };
        let _ = conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;");

        while let Some(record) = rx.recv().await {
            if let Err(e) = Self::insert_record(&conn, &record) {
                tracing::warn!("写入请求日志失败: {}", e);
            }
        }
        tracing::info!("Trace 写入任务退出（channel 已关闭）");
    }

    /// 向数据库插入一条请求记录
    fn insert_record(conn: &Connection, record: &TraceRecord) -> rusqlite::Result<()> {
        let ts = chrono::Utc::now().to_rfc3339();
        let ts_epoch = chrono::Utc::now().timestamp();

        let tx = conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO request_logs (ts, ts_epoch, path, model, is_stream, final_status,
             final_credential_id, duration_ms, input_tokens, output_tokens, total_attempts, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                ts,
                ts_epoch,
                record.path,
                record.model,
                record.is_stream as i32,
                record.final_status,
                record.final_credential_id as i64,
                record.duration_ms,
                record.input_tokens,
                record.output_tokens,
                record.total_attempts,
                record.error,
            ],
        )?;

        let log_id = tx.last_insert_rowid();

        for attempt in &record.attempts {
            tx.execute(
                "INSERT INTO request_attempts (log_id, try_number, credential_id, status_code,
                 outcome, duration_ms, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    log_id,
                    attempt.try_number,
                    attempt.credential_id as i64,
                    attempt.status_code,
                    Self::outcome_to_str(attempt.outcome),
                    attempt.duration_ms,
                    attempt.error,
                ],
            )?;
        }

        tx.commit()
    }

    fn outcome_to_str(outcome: AttemptOutcome) -> &'static str {
        match outcome {
            AttemptOutcome::Success => "success",
            AttemptOutcome::QuotaExhausted => "quota_exhausted",
            AttemptOutcome::AccountThrottled => "account_throttled",
            AttemptOutcome::AuthFailed => "auth_failed",
            AttemptOutcome::Transient => "transient",
            AttemptOutcome::NetworkError => "network_error",
            AttemptOutcome::BadRequest => "bad_request",
            AttemptOutcome::Unknown => "unknown",
        }
    }

    /// 异步写入一条请求记录（非阻塞，满时丢弃）
    pub async fn record(&self, record: TraceRecord) {
        if self.tx.try_send(record).is_err() {
            tracing::debug!("Trace channel 已满或关闭，丢弃日志");
        }
    }

    /// 查询最近的请求日志（带 attempts 明细）
    pub fn query_logs(
        &self,
        limit: u32,
        before_ts_epoch: Option<i64>,
        status_filter: Option<i32>,
        credential_filter: Option<u64>,
    ) -> anyhow::Result<Vec<RequestLogSummary>> {
        let conn =
            Connection::open_with_flags(&self.db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;

        // 动态构建查询
        let mut sql = String::from(
            "SELECT id, ts, ts_epoch, path, model, is_stream, final_status, final_credential_id,
             duration_ms, input_tokens, output_tokens, total_attempts, error
             FROM request_logs WHERE 1=1",
        );

        if before_ts_epoch.is_some() {
            sql.push_str(" AND ts_epoch < ?");
        }
        if status_filter.is_some() {
            sql.push_str(" AND final_status = ?");
        }
        if credential_filter.is_some() {
            sql.push_str(" AND final_credential_id = ?");
        }
        sql.push_str(" ORDER BY ts_epoch DESC LIMIT ?");

        // 按顺序收集参数
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(ts) = before_ts_epoch {
            param_values.push(Box::new(ts));
        }
        if let Some(s) = status_filter {
            param_values.push(Box::new(s));
        }
        if let Some(c) = credential_filter {
            param_values.push(Box::new(c as i64));
        }
        param_values.push(Box::new(limit as i64));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let logs: Vec<RequestLogSummary> = stmt
            .query_map(params_ref.as_slice(), |row| {
                let id: i64 = row.get(0)?;
                let ts: String = row.get(1)?;
                let ts_epoch: i64 = row.get(2)?;
                let path: String = row.get(3)?;
                let model: Option<String> = row.get(4)?;
                let is_stream: i32 = row.get(5)?;
                let final_status: i32 = row.get(6)?;
                let final_credential_id: i64 = row.get(7)?;
                let duration_ms: i64 = row.get(8)?;
                let input_tokens: Option<i32> = row.get(9)?;
                let output_tokens: Option<i32> = row.get(10)?;
                let total_attempts: i32 = row.get(11)?;
                let error: Option<String> = row.get(12)?;

                // 查询 attempts
                let attempts = Self::query_attempts(&conn, id).unwrap_or_default();

                Ok(RequestLogSummary {
                    id,
                    ts,
                    ts_epoch,
                    path,
                    model,
                    is_stream: is_stream != 0,
                    final_status,
                    final_credential_id: final_credential_id as u64,
                    duration_ms,
                    input_tokens,
                    output_tokens,
                    total_attempts,
                    error,
                    attempts,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(logs)
    }

    /// 查询单条日志的所有重试尝试
    fn query_attempts(conn: &Connection, log_id: i64) -> rusqlite::Result<Vec<AttemptSummary>> {
        let mut stmt = conn.prepare(
            "SELECT try_number, credential_id, status_code, outcome, duration_ms, error
             FROM request_attempts WHERE log_id = ?1 ORDER BY try_number",
        )?;

        stmt.query_map(params![log_id], |row| {
            Ok(AttemptSummary {
                try_number: row.get(0)?,
                credential_id: row.get::<_, i64>(1)? as u64,
                status_code: row.get(2)?,
                outcome: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                duration_ms: row.get(4)?,
                error: row.get(5)?,
            })
        })?
        .collect()
    }

    /// 查询符合筛选条件的日志总数（不受分页游标和 limit 影响）
    pub fn count_logs(
        &self,
        status_filter: Option<i32>,
        credential_filter: Option<u64>,
    ) -> anyhow::Result<usize> {
        let conn =
            Connection::open_with_flags(&self.db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;

        let mut sql = String::from("SELECT COUNT(*) FROM request_logs WHERE 1=1");
        if status_filter.is_some() {
            sql.push_str(" AND final_status = ?");
        }
        if credential_filter.is_some() {
            sql.push_str(" AND final_credential_id = ?");
        }

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(s) = status_filter {
            param_values.push(Box::new(s));
        }
        if let Some(c) = credential_filter {
            param_values.push(Box::new(c as i64));
        }
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let count: i64 = conn.query_row(&sql, params_ref.as_slice(), |row| row.get(0))?;
        Ok(count as usize)
    }

    /// 清空所有请求日志（含 attempts，依赖 ON DELETE CASCADE）
    pub fn delete_all_logs(&self) -> anyhow::Result<u64> {
        let conn =
            Connection::open_with_flags(&self.db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.execute("DELETE FROM request_attempts", params![])?;
        let count = conn.execute("DELETE FROM request_logs", params![])?;
        tracing::warn!("清空了 {} 条请求日志", count);
        Ok(count as u64)
    }

    /// 清理过期日志
    pub fn cleanup(&self) -> anyhow::Result<usize> {
        let conn = Connection::open(&self.db_path)?;
        let cutoff = chrono::Utc::now().timestamp() - (self.retention_days as i64 * 86400);

        let count1 = conn.execute(
            "DELETE FROM request_attempts WHERE log_id IN
             (SELECT id FROM request_logs WHERE ts_epoch < ?1)",
            params![cutoff],
        )?;
        let count2 = conn.execute(
            "DELETE FROM request_logs WHERE ts_epoch < ?1",
            params![cutoff],
        )?;

        if count1 + count2 > 0 {
            tracing::info!("清理了 {} 条过期请求日志", count1 + count2);
        }
        Ok(count1 + count2)
    }
}
