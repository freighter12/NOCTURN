//! Change journal persistence (transactions + changes tables).

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::{ChangeOp, Original};
use crate::db::{now, Db};
use crate::error::{AppError, AppResult};

pub mod status {
    pub const PENDING: &str = "pending";
    pub const APPLIED: &str = "applied";
    pub const UNCHANGED: &str = "unchanged";
    pub const FAILED: &str = "failed";
    pub const SKIPPED: &str = "skipped";
    pub const UNKNOWN: &str = "unknown";
    pub const ROLLED_BACK: &str = "rolled_back";
    pub const ROLLBACK_FAILED: &str = "rollback_failed";
    pub const NOT_REVERSIBLE: &str = "not_reversible";
}

pub mod tx_status {
    pub const IN_PROGRESS: &str = "in_progress";
    pub const COMPLETED: &str = "completed";
    pub const PARTIAL: &str = "partial";
    pub const FAILED: &str = "failed";
    pub const NOTHING_TO_DO: &str = "nothing_to_do";
    pub const INTERRUPTED: &str = "interrupted";
    pub const ROLLED_BACK: &str = "rolled_back";
    pub const PARTIALLY_ROLLED_BACK: &str = "partially_rolled_back";
}

#[derive(Debug, Clone, Serialize)]
pub struct TxSummary {
    pub id: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub source: String,
    pub label: String,
    pub status: String,
    pub restore_point: Option<String>,
    pub backup_path: Option<String>,
    pub total: i64,
    pub applied: i64,
    pub failed: i64,
    pub rolled_back: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeRow {
    pub id: i64,
    pub tx_id: String,
    pub tweak_id: String,
    pub target: String,
    pub op: ChangeOp,
    pub original: Option<Original>,
    pub status: String,
    pub error: Option<String>,
    pub applied_at: Option<String>,
    pub rolled_back_at: Option<String>,
}

pub fn create_tx(db: &Db, id: &str, source: &str, label: &str) -> AppResult<()> {
    db.conn().execute(
        "INSERT INTO transactions(id, created_at, source, label, status) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, now(), source, label, tx_status::IN_PROGRESS],
    )?;
    Ok(())
}

pub fn set_tx_backup(db: &Db, id: &str, path: &str) -> AppResult<()> {
    db.conn().execute("UPDATE transactions SET backup_path = ?2 WHERE id = ?1", params![id, path])?;
    Ok(())
}

pub fn set_tx_restore_point(db: &Db, id: &str, text: &str) -> AppResult<()> {
    db.conn()
        .execute("UPDATE transactions SET restore_point = ?2 WHERE id = ?1", params![id, text])?;
    Ok(())
}

pub fn finish_tx(db: &Db, id: &str, status: &str) -> AppResult<()> {
    db.conn().execute(
        "UPDATE transactions SET status = ?2, finished_at = ?3 WHERE id = ?1",
        params![id, status, now()],
    )?;
    Ok(())
}

/// Insert a change *before* touching the system (write-ahead).
pub fn insert_change(
    db: &Db,
    tx_id: &str,
    tweak_id: &str,
    op: &ChangeOp,
    original: &Original,
    status: &str,
) -> AppResult<i64> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO changes(tx_id, tweak_id, target, op_json, original_json, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            tx_id,
            tweak_id,
            op.target(),
            serde_json::to_string(op)?,
            serde_json::to_string(original)?,
            status
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_original(db: &Db, change_id: i64, original: &Original) -> AppResult<()> {
    db.conn().execute(
        "UPDATE changes SET original_json = ?2 WHERE id = ?1",
        params![change_id, serde_json::to_string(original)?],
    )?;
    Ok(())
}

pub fn mark_applied(db: &Db, change_id: i64, status: &str, error: Option<&str>) -> AppResult<()> {
    db.conn().execute(
        "UPDATE changes SET status = ?2, error = ?3, applied_at = ?4 WHERE id = ?1",
        params![change_id, status, error, now()],
    )?;
    Ok(())
}

pub fn mark_rolled_back(db: &Db, change_id: i64, status: &str, error: Option<&str>) -> AppResult<()> {
    db.conn().execute(
        "UPDATE changes SET status = ?2, error = ?3, rolled_back_at = ?4 WHERE id = ?1",
        params![change_id, status, error, now()],
    )?;
    Ok(())
}

fn row_to_change(r: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, String, String, String, String, Option<String>, String, Option<String>, Option<String>, Option<String>)> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
    ))
}

pub fn changes_for(db: &Db, tx_id: &str) -> AppResult<Vec<ChangeRow>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, tx_id, tweak_id, target, op_json, original_json, status, error, applied_at, rolled_back_at
         FROM changes WHERE tx_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([tx_id], row_to_change)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, tx_id, tweak_id, target, op_json, original_json, status, error, applied_at, rolled_back_at) = row?;
        out.push(ChangeRow {
            id,
            tx_id,
            tweak_id,
            target,
            op: serde_json::from_str(&op_json)?,
            original: original_json.as_deref().map(serde_json::from_str).transpose()?,
            status,
            error,
            applied_at,
            rolled_back_at,
        });
    }
    Ok(out)
}

pub fn list_tx(db: &Db, limit: i64) -> AppResult<Vec<TxSummary>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT t.id, t.created_at, t.finished_at, t.source, t.label, t.status, t.restore_point, t.backup_path,
                COUNT(c.id),
                SUM(CASE WHEN c.status IN ('applied') THEN 1 ELSE 0 END),
                SUM(CASE WHEN c.status IN ('failed','rollback_failed') THEN 1 ELSE 0 END),
                SUM(CASE WHEN c.status = 'rolled_back' THEN 1 ELSE 0 END)
         FROM transactions t LEFT JOIN changes c ON c.tx_id = t.id
         GROUP BY t.id ORDER BY t.created_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(TxSummary {
            id: r.get(0)?,
            created_at: r.get(1)?,
            finished_at: r.get(2)?,
            source: r.get(3)?,
            label: r.get(4)?,
            status: r.get(5)?,
            restore_point: r.get(6)?,
            backup_path: r.get(7)?,
            total: r.get(8)?,
            applied: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
            failed: r.get::<_, Option<i64>>(10)?.unwrap_or(0),
            rolled_back: r.get::<_, Option<i64>>(11)?.unwrap_or(0),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_tx(db: &Db, id: &str) -> AppResult<TxSummary> {
    list_tx(db, 10_000)?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::NotFound(format!("transaction {id}")))
}

pub fn tx_exists(db: &Db, id: &str) -> AppResult<bool> {
    Ok(db
        .conn()
        .query_row("SELECT 1 FROM transactions WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// Called at startup: anything left `pending` was interrupted mid-apply.
pub fn recover_interrupted(db: &Db) -> AppResult<usize> {
    let conn = db.conn();
    let n = conn.execute(
        "UPDATE changes SET status = 'unknown', error = 'NOCTURN closed before this change was confirmed' WHERE status = 'pending'",
        [],
    )?;
    conn.execute(
        "UPDATE transactions SET status = ?1, finished_at = ?2 WHERE status = ?3",
        params![tx_status::INTERRUPTED, now(), tx_status::IN_PROGRESS],
    )?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tweaks::{ChangeOp, Original};

    #[test]
    fn write_ahead_and_recovery() {
        let db = Db::open_in_memory().unwrap();
        create_tx(&db, "t1", "test", "Test").unwrap();
        let id = insert_change(&db, "t1", "x", &ChangeOp::FlushDns, &Original::Nothing, status::PENDING).unwrap();
        // Simulate a crash: nothing marked applied, process restarts.
        assert_eq!(recover_interrupted(&db).unwrap(), 1);
        let c = changes_for(&db, "t1").unwrap();
        assert_eq!(c[0].id, id);
        assert_eq!(c[0].status, status::UNKNOWN);
        assert_eq!(get_tx(&db, "t1").unwrap().status, tx_status::INTERRUPTED);
    }
}
