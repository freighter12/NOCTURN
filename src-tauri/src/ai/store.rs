//! Conversation persistence.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::Message;
use crate::db::{now, Db};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredMessage {
    pub id: i64,
    pub created_at: String,
    #[serde(flatten)]
    pub message: Message,
}

pub fn create(db: &Db, title: &str, provider: &str, model: &str) -> AppResult<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let t = now();
    db.conn().execute(
        "INSERT INTO conversations(id, title, provider, model, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id, title, provider, model, t],
    )?;
    Ok(id)
}

pub fn exists(db: &Db, id: &str) -> AppResult<bool> {
    Ok(db.conn().query_row("SELECT 1 FROM conversations WHERE id = ?1", [id], |_| Ok(())).optional()?.is_some())
}

pub fn append(db: &Db, conv: &str, m: &Message) -> AppResult<i64> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO messages(conversation_id, role, content_json, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![conv, m.role, serde_json::to_string(m)?, now()],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute("UPDATE conversations SET updated_at = ?2 WHERE id = ?1", params![conv, now()])?;
    Ok(id)
}

pub fn messages(db: &Db, conv: &str) -> AppResult<Vec<StoredMessage>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, content_json, created_at FROM messages WHERE conversation_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([conv], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, json, created_at) = row?;
        match serde_json::from_str::<Message>(&json) {
            Ok(message) => out.push(StoredMessage { id, created_at, message }),
            Err(e) => tracing::warn!("skipping unreadable message {id}: {e}"),
        }
    }
    Ok(out)
}

pub fn list(db: &Db) -> AppResult<Vec<ConversationSummary>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, title, provider, model, created_at, updated_at FROM conversations ORDER BY updated_at DESC LIMIT 200",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ConversationSummary {
            id: r.get(0)?,
            title: r.get(1)?,
            provider: r.get(2)?,
            model: r.get(3)?,
            created_at: r.get(4)?,
            updated_at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn rename(db: &Db, id: &str, title: &str) -> AppResult<()> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::Invalid("title can't be empty".into()));
    }
    db.conn().execute(
        "UPDATE conversations SET title = ?2 WHERE id = ?1",
        params![id, crate::util::clamp(title, 120)],
    )?;
    Ok(())
}

pub fn delete(db: &Db, id: &str) -> AppResult<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM messages WHERE conversation_id = ?1", [id])?;
    conn.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_model(db: &Db, id: &str, provider: &str, model: &str) -> AppResult<()> {
    db.conn().execute(
        "UPDATE conversations SET provider = ?2, model = ?3 WHERE id = ?1",
        params![id, provider, model],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud() {
        let db = Db::open_in_memory().unwrap();
        let id = create(&db, "Test", "anthropic", "claude-opus-5-5").unwrap();
        append(&db, &id, &Message::user_text("hello")).unwrap();
        let m = messages(&db, &id).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(list(&db).unwrap().len(), 1);
        rename(&db, &id, "Renamed").unwrap();
        assert_eq!(list(&db).unwrap()[0].title, "Renamed");
        delete(&db, &id).unwrap();
        assert!(list(&db).unwrap().is_empty());
    }
}
