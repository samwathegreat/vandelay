/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};

const MAILBOX: &str = "mailbox";
const EMAIL: &str = "email";

pub fn insert_mailbox(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    local_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_id_imap
         (source_id, type_name, folder, uidvalidity, uid, local_id)
         VALUES (?1, ?2, ?3, 0, 0, ?4)",
        params![source_id, MAILBOX, folder, local_id],
    )?;
    Ok(())
}

pub fn insert_email(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    uidvalidity: u32,
    uid: u32,
    local_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_id_imap
         (source_id, type_name, folder, uidvalidity, uid, local_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![source_id, EMAIL, folder, uidvalidity, uid, local_id],
    )?;
    Ok(())
}

pub fn local_for_mailbox(
    conn: &Connection,
    source_id: i64,
    folder: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT local_id FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3
           AND uidvalidity = 0 AND uid = 0",
        params![source_id, MAILBOX, folder],
        |row| row.get(0),
    )
    .optional()
}

pub fn local_for_email(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    uidvalidity: u32,
    uid: u32,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT local_id FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2
           AND folder = ?3 AND uidvalidity = ?4 AND uid = ?5",
        params![source_id, EMAIL, folder, uidvalidity, uid],
        |row| row.get(0),
    )
    .optional()
}

pub fn mailbox_folders(
    conn: &Connection,
    source_id: i64,
) -> Result<HashMap<String, i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT folder, local_id FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2",
    )?;
    let rows = stmt.query_map(params![source_id, MAILBOX], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = HashMap::new();
    for r in rows {
        let (f, l) = r?;
        map.insert(f, l);
    }
    Ok(map)
}

pub fn email_uids_in_folder(
    conn: &Connection,
    source_id: i64,
    folder: &str,
) -> Result<HashMap<(u32, u32), i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT uidvalidity, uid, local_id FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3",
    )?;
    let rows = stmt.query_map(params![source_id, EMAIL, folder], |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, u32>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut map = HashMap::new();
    for r in rows {
        let (v, u, l) = r?;
        map.insert((v, u), l);
    }
    Ok(map)
}

pub fn email_observation_count(
    conn: &Connection,
    source_id: i64,
    local_id: i64,
) -> Result<u64, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2 AND local_id = ?3",
        params![source_id, EMAIL, local_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count as u64)
}

pub fn email_folders_for_local(
    conn: &Connection,
    source_id: i64,
    local_id: i64,
) -> Result<HashSet<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT folder FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2 AND local_id = ?3",
    )?;
    let rows = stmt.query_map(params![source_id, EMAIL, local_id], |row| {
        row.get::<_, String>(0)
    })?;
    let mut out = HashSet::new();
    for row in rows {
        out.insert(row?);
    }
    Ok(out)
}

pub fn delete_mailbox(
    conn: &Connection,
    source_id: i64,
    folder: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3
           AND uidvalidity = 0 AND uid = 0",
        params![source_id, MAILBOX, folder],
    )?;
    Ok(())
}

pub fn delete_email(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    uidvalidity: u32,
    uid: u32,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2
           AND folder = ?3 AND uidvalidity = ?4 AND uid = ?5",
        params![source_id, EMAIL, folder, uidvalidity, uid],
    )?;
    Ok(())
}

pub fn delete_all_emails_in_folder(
    conn: &Connection,
    source_id: i64,
    folder: &str,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3",
        params![source_id, EMAIL, folder],
    )
}

pub fn email_uids_at_validity(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    uidvalidity: u32,
) -> Result<Vec<u32>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT uid FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3
           AND uidvalidity = ?4
         ORDER BY uid",
    )?;
    let rows = stmt.query_map(params![source_id, EMAIL, folder, uidvalidity], |row| {
        row.get::<_, u32>(0)
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn folders_with_emails(
    conn: &Connection,
    source_id: i64,
) -> Result<HashSet<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT folder FROM sync_id_imap
         WHERE source_id = ?1 AND type_name = ?2",
    )?;
    let rows = stmt.query_map(params![source_id, EMAIL], |row| row.get::<_, String>(0))?;
    let mut out = HashSet::new();
    for r in rows {
        out.insert(r?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init;
    use crate::db::sources::{SourceKey, upsert_source};

    fn setup() -> (Connection, i64) {
        let c = Connection::open_in_memory().unwrap();
        init::apply_schema(&c).unwrap();
        let sid = upsert_source(
            &c,
            &SourceKey {
                kind: "imap".to_owned(),
                session_url: "imaps://host:993".to_owned(),
                account_id: "alice".to_owned(),
            },
            None,
            "alice",
        )
        .unwrap();
        (c, sid)
    }

    #[test]
    fn mailbox_roundtrip() {
        let (c, sid) = setup();
        insert_mailbox(&c, sid, "INBOX", 7).unwrap();
        insert_mailbox(&c, sid, "INBOX/Sub", 8).unwrap();
        assert_eq!(local_for_mailbox(&c, sid, "INBOX").unwrap(), Some(7));
        let all = mailbox_folders(&c, sid).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("INBOX/Sub"), Some(&8));
        delete_mailbox(&c, sid, "INBOX").unwrap();
        assert_eq!(local_for_mailbox(&c, sid, "INBOX").unwrap(), None);
    }

    #[test]
    fn email_roundtrip() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", 12345, 100, 1).unwrap();
        insert_email(&c, sid, "INBOX", 12345, 101, 2).unwrap();
        insert_email(&c, sid, "Sent", 7777, 1, 3).unwrap();
        assert_eq!(
            local_for_email(&c, sid, "INBOX", 12345, 100).unwrap(),
            Some(1)
        );
        let inbox = email_uids_in_folder(&c, sid, "INBOX").unwrap();
        assert_eq!(inbox.len(), 2);
        assert_eq!(inbox.get(&(12345, 100)), Some(&1));
        let folders = folders_with_emails(&c, sid).unwrap();
        assert!(folders.contains("INBOX"));
        assert!(folders.contains("Sent"));
    }

    #[test]
    fn one_email_can_have_many_imap_observations() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", 1, 10, 42).unwrap();
        insert_email(&c, sid, "[Gmail]/All Mail", 2, 20, 42).unwrap();
        insert_email(&c, sid, "Project", 3, 30, 42).unwrap();

        assert_eq!(email_observation_count(&c, sid, 42).unwrap(), 3);
        let folders = email_folders_for_local(&c, sid, 42).unwrap();
        assert_eq!(folders.len(), 3);
        assert!(folders.contains("INBOX"));
        assert!(folders.contains("[Gmail]/All Mail"));
        assert!(folders.contains("Project"));

        delete_email(&c, sid, "INBOX", 1, 10).unwrap();
        assert_eq!(email_observation_count(&c, sid, 42).unwrap(), 2);
    }

    #[test]
    fn deleting_one_observation_preserves_the_other_observations() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", 10, 101, 42).unwrap();
        insert_email(&c, sid, "[Gmail]/All Mail", 20, 202, 42).unwrap();
        insert_email(&c, sid, "Project", 30, 303, 42).unwrap();

        delete_email(&c, sid, "INBOX", 10, 101).unwrap();

        assert_eq!(email_observation_count(&c, sid, 42).unwrap(), 2);
        assert_eq!(
            local_for_email(&c, sid, "[Gmail]/All Mail", 20, 202).unwrap(),
            Some(42)
        );
        assert_eq!(
            local_for_email(&c, sid, "Project", 30, 303).unwrap(),
            Some(42)
        );
    }

    #[test]
    fn duplicate_uids_in_one_mailbox_can_share_a_canonical_email() {
        let (c, sid) = setup();
        insert_email(&c, sid, "[Gmail]/Important", 55, 1001, 42).unwrap();
        insert_email(&c, sid, "[Gmail]/Important", 55, 1002, 42).unwrap();

        assert_eq!(email_observation_count(&c, sid, 42).unwrap(), 2);
        let folder = email_uids_in_folder(&c, sid, "[Gmail]/Important").unwrap();
        assert_eq!(folder.get(&(55, 1001)), Some(&42));
        assert_eq!(folder.get(&(55, 1002)), Some(&42));

        delete_email(&c, sid, "[Gmail]/Important", 55, 1001).unwrap();
        assert_eq!(email_observation_count(&c, sid, 42).unwrap(), 1);
        assert_eq!(
            local_for_email(&c, sid, "[Gmail]/Important", 55, 1002).unwrap(),
            Some(42)
        );
    }

    #[test]
    fn delete_all_emails_in_folder_wipes_only_that_folder() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", 1, 10, 1).unwrap();
        insert_email(&c, sid, "INBOX", 1, 11, 2).unwrap();
        insert_email(&c, sid, "Sent", 1, 10, 3).unwrap();
        let removed = delete_all_emails_in_folder(&c, sid, "INBOX").unwrap();
        assert_eq!(removed, 2);
        assert!(email_uids_in_folder(&c, sid, "INBOX").unwrap().is_empty());
        assert_eq!(email_uids_in_folder(&c, sid, "Sent").unwrap().len(), 1);
    }

    #[test]
    fn re_insert_with_same_local_id_replaces() {
        let (c, sid) = setup();
        insert_mailbox(&c, sid, "A", 1).unwrap();
        insert_mailbox(&c, sid, "B", 1).unwrap();
        assert_eq!(local_for_mailbox(&c, sid, "A").unwrap(), None);
        assert_eq!(local_for_mailbox(&c, sid, "B").unwrap(), Some(1));
        let all = mailbox_folders(&c, sid).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn pk_composite_is_per_type_per_folder_per_uid() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", 1, 10, 1).unwrap();
        insert_email(&c, sid, "INBOX", 1, 10, 1).unwrap();
        assert_eq!(email_uids_in_folder(&c, sid, "INBOX").unwrap().len(), 1);
    }

    #[test]
    fn mailbox_and_email_namespaces_do_not_collide_on_local_id() {
        let (c, sid) = setup();
        insert_mailbox(&c, sid, "INBOX", 42).unwrap();
        insert_email(&c, sid, "INBOX", 1, 1, 42).unwrap();
    }
}
