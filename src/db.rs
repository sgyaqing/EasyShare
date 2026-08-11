use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};

use crate::models::{Message, MessageKind};

pub fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            hostname TEXT NOT NULL,
            ip TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('text','image','file')),
            content TEXT,
            filename TEXT,
            orig_name TEXT,
            created_at INTEGER NOT NULL,
            recalled INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_messages_id ON messages(id DESC);",
    )
    .context("failed to initialize database schema")?;
    Ok(())
}

pub fn insert_message(
    conn: &Connection,
    hostname: &str,
    ip: &str,
    kind: MessageKind,
    content: Option<&str>,
    filename: Option<&str>,
    orig_name: Option<&str>,
) -> Result<Message> {
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    conn.execute(
        "INSERT INTO messages (hostname, ip, kind, content, filename, orig_name, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![hostname, ip, kind.as_str(), content, filename, orig_name, created_at],
    )?;
    let id = conn.last_insert_rowid();
    Ok(Message {
        id,
        hostname: hostname.to_string(),
        ip: ip.to_string(),
        kind,
        content: content.map(str::to_string),
        filename: filename.map(str::to_string),
        orig_name: orig_name.map(str::to_string),
        created_at,
        recalled: false,
    })
}

/// Keyset pagination: newest first, `before` excludes that id and everything newer.
/// Returns up to `limit` messages in ascending id order (oldest of the page first).
pub fn fetch_messages(
    conn: &Connection,
    before: Option<i64>,
    limit: i64,
) -> Result<(Vec<Message>, bool)> {
    // Fetch limit+1 to determine has_more.
    let sql = match before {
        Some(_) => {
            "SELECT id, hostname, ip, kind, content, filename, orig_name, created_at, recalled
             FROM messages WHERE id < ?1 ORDER BY id DESC LIMIT ?2"
        }
        None => {
            "SELECT id, hostname, ip, kind, content, filename, orig_name, created_at, recalled
             FROM messages ORDER BY id DESC LIMIT ?1"
        }
    };
    let mut stmt = conn.prepare(sql)?;
    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<Message> {
        let kind_str: String = row.get(3)?;
        let recalled: i64 = row.get(8)?;
        Ok(Message {
            id: row.get(0)?,
            hostname: row.get(1)?,
            ip: row.get(2)?,
            kind: MessageKind::from_str(&kind_str).unwrap_or(MessageKind::Text),
            content: row.get(4)?,
            filename: row.get(5)?,
            orig_name: row.get(6)?,
            created_at: row.get(7)?,
            recalled: recalled != 0,
        })
    };
    let rows: Vec<Message> = match before {
        Some(b) => stmt
            .query_map(params![b, limit + 1], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => stmt
            .query_map(params![limit + 1], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    let has_more = rows.len() as i64 > limit;
    let mut page: Vec<Message> = rows.into_iter().take(limit as usize).collect();
    page.reverse(); // ascending order for display
    Ok((page, has_more))
}

/// Recall a message; only the original sender (hostname + ip match) may recall.
pub fn recall_message(
    conn: &Connection,
    id: i64,
    hostname: &str,
    ip: &str,
) -> Result<bool> {
    let owner: Option<(String, String)> = conn
        .query_row(
            "SELECT hostname, ip FROM messages WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    match owner {
        None => bail!("message not found"),
        Some((h, p)) if h == hostname && p == ip => {
            conn.execute(
                "UPDATE messages SET recalled = 1 WHERE id = ?1",
                params![id],
            )?;
            Ok(true)
        }
        Some(_) => Ok(false),
    }
}

pub fn get_message(conn: &Connection, id: i64) -> Result<Option<Message>> {
    let mut stmt = conn.prepare(
        "SELECT id, hostname, ip, kind, content, filename, orig_name, created_at, recalled
         FROM messages WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        let kind_str: String = row.get(3)?;
        let recalled: i64 = row.get(8)?;
        Ok(Message {
            id: row.get(0)?,
            hostname: row.get(1)?,
            ip: row.get(2)?,
            kind: MessageKind::from_str(&kind_str).unwrap_or(MessageKind::Text),
            content: row.get(4)?,
            filename: row.get(5)?,
            orig_name: row.get(6)?,
            created_at: row.get(7)?,
            recalled: recalled != 0,
        })
    })?;
    match rows.next() {
        Some(Ok(m)) => Ok(Some(m)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init(&conn).unwrap();
        conn
    }

    fn insert_n(conn: &Connection, n: i64) {
        for i in 0..n {
            insert_message(
                conn,
                "host",
                "127.0.0.1",
                MessageKind::Text,
                Some(&format!("msg {i}")),
                None,
                None,
            )
            .unwrap();
        }
    }

    #[test]
    fn insert_and_fetch_latest() {
        let conn = setup();
        insert_n(&conn, 3);
        let (msgs, has_more) = fetch_messages(&conn, None, 10).unwrap();
        assert_eq!(msgs.len(), 3);
        assert!(!has_more);
        assert_eq!(msgs[0].content.as_deref(), Some("msg 0"));
        assert_eq!(msgs[2].content.as_deref(), Some("msg 2"));
    }

    #[test]
    fn pagination_pages_of_ten() {
        let conn = setup();
        insert_n(&conn, 25);
        // Page 1: latest 10 (ids 16..=25)
        let (p1, more1) = fetch_messages(&conn, None, 10).unwrap();
        assert_eq!(p1.len(), 10);
        assert!(more1);
        assert_eq!(p1[9].id, 25);
        // Page 2: before id 16 → ids 6..=15
        let (p2, more2) = fetch_messages(&conn, Some(p1[0].id), 10).unwrap();
        assert_eq!(p2.len(), 10);
        assert!(more2);
        assert_eq!(p2[9].id, 15);
        // Page 3: ids 1..=5
        let (p3, more3) = fetch_messages(&conn, Some(p2[0].id), 10).unwrap();
        assert_eq!(p3.len(), 5);
        assert!(!more3);
        assert_eq!(p3[0].id, 1);
        // Page 4: empty
        let (p4, more4) = fetch_messages(&conn, Some(p3[0].id), 10).unwrap();
        assert!(p4.is_empty());
        assert!(!more4);
    }

    #[test]
    fn recall_by_owner_succeeds() {
        let conn = setup();
        let m = insert_message(
            &conn,
            "alice-pc",
            "192.168.1.2",
            MessageKind::Text,
            Some("hello"),
            None,
            None,
        )
        .unwrap();
        assert!(recall_message(&conn, m.id, "alice-pc", "192.168.1.2").unwrap());
        let fetched = get_message(&conn, m.id).unwrap().unwrap();
        assert!(fetched.recalled);
    }

    #[test]
    fn recall_by_other_fails() {
        let conn = setup();
        let m = insert_message(
            &conn,
            "alice-pc",
            "192.168.1.2",
            MessageKind::Text,
            Some("hello"),
            None,
            None,
        )
        .unwrap();
        assert!(!recall_message(&conn, m.id, "bob-pc", "192.168.1.3").unwrap());
        let fetched = get_message(&conn, m.id).unwrap().unwrap();
        assert!(!fetched.recalled);
    }

    #[test]
    fn recall_missing_message_errors() {
        let conn = setup();
        assert!(recall_message(&conn, 999, "a", "1.1.1.1").is_err());
    }

    #[test]
    fn recalled_rows_still_paginate() {
        let conn = setup();
        insert_n(&conn, 12);
        recall_message(&conn, 11, "host", "127.0.0.1").unwrap();
        let (p1, more) = fetch_messages(&conn, None, 10).unwrap();
        assert_eq!(p1.len(), 10);
        assert!(more);
        assert!(p1.iter().any(|m| m.id == 11 && m.recalled));
    }
}
