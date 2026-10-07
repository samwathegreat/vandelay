/*
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Consolidation {
    pub groups: u64,
    pub removed_rows: u64,
    pub remapped_imap_observations: u64,
    pub canonical_mailboxes: u64,
    pub removed_mailboxes: u64,
}

/// Collapse duplicate Email rows that reference the same exact RFC822 blob.
///
/// The lowest Email id is retained as the canonical row. IMAP observations are
/// repointed to it, mailbox membership and keywords are unioned, and duplicate
/// rows are removed. Takeout identity is retained when it can be repointed
/// without violating its one-local-object constraint.
///
/// This is deliberately explicit rather than an automatic schema migration:
/// callers can checkpoint a real archive before changing its logical model.
pub fn consolidate_by_blob(conn: &mut Connection) -> rusqlite::Result<Consolidation> {
    let tx = conn.transaction()?;
    let groups = {
        let mut stmt = tx.prepare(
            "SELECT blob_id FROM emails GROUP BY blob_id HAVING COUNT(*) > 1 ORDER BY blob_id",
        )?;
        stmt.query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut out = Consolidation::default();
    canonicalize_gmail_special_mailboxes(&tx, &mut out)?;
    for blob_id in groups {
        let mut ids = Vec::new();
        let mut keywords = BTreeSet::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id, mailbox_ids, keywords FROM emails WHERE blob_id = ?1 ORDER BY id",
            )?;
            let rows = stmt.query_map(params![blob_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (id, _mailbox_json, keyword_json) = row?;
                ids.push(id);
                for value in json_array(&keyword_json)? {
                    if let Some(keyword) = value.as_str() {
                        keywords.insert(keyword.to_owned());
                    }
                }
            }
        }
        let Some((&canonical, losers)) = ids.split_first() else {
            continue;
        };

        for loser in losers {
            out.remapped_imap_observations += tx.execute(
                "UPDATE sync_id_imap SET local_id = ?1
                 WHERE type_name = 'email' AND local_id = ?2",
                params![canonical, loser],
            )? as u64;

            // A Takeout source normally has one seed row for a physical Email.
            // Preserve it if the canonical row does not already have a mapping
            // for that same source; otherwise the duplicate mapping is stale.
            let takeout: Vec<(i64, String)> = {
                let mut stmt = tx.prepare(
                    "SELECT source_id, source_obj_id FROM sync_id_takeout
                     WHERE type_name = 'email' AND local_id = ?1",
                )?;
                stmt.query_map(params![loser], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?
            };
            for (source_id, source_obj_id) in takeout {
                let occupied: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sync_id_takeout
                     WHERE source_id = ?1 AND type_name = 'email' AND local_id = ?2)",
                    params![source_id, canonical],
                    |r| r.get(0),
                )?;
                if occupied {
                    tx.execute(
                        "DELETE FROM sync_id_takeout
                         WHERE source_id = ?1 AND type_name = 'email' AND source_obj_id = ?2",
                        params![source_id, source_obj_id],
                    )?;
                } else {
                    tx.execute(
                        "UPDATE sync_id_takeout SET local_id = ?1
                         WHERE source_id = ?2 AND type_name = 'email' AND source_obj_id = ?3",
                        params![canonical, source_id, source_obj_id],
                    )?;
                }
            }

            // A target mapping for a duplicate logical row cannot be trusted
            // after consolidation. The next export must establish canonical
            // identity again rather than inherit a duplicate target object.
            tx.execute(
                "DELETE FROM export_id_jmap WHERE type_name = 'email' AND local_id = ?1",
                params![loser],
            )?;
            tx.execute("DELETE FROM emails WHERE id = ?1", params![loser])?;
            out.removed_rows += 1;
        }

        let mailboxes = observed_mailboxes(&tx, canonical)?;
        let mailbox_json = serde_json::to_string(&mailboxes)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let keyword_json = serde_json::to_string(&keywords.into_iter().collect::<Vec<_>>())
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        tx.execute(
            "UPDATE emails SET mailbox_ids = ?1, keywords = ?2 WHERE id = ?3",
            params![mailbox_json, keyword_json, canonical],
        )?;
        out.groups += 1;
    }

    tx.commit()?;
    Ok(out)
}

fn canonicalize_gmail_special_mailboxes(
    conn: &Connection,
    out: &mut Consolidation,
) -> rusqlite::Result<()> {
    // Existing archives may contain an older canonical special mailbox plus a
    // second ordinary mailbox created by a later Gmail IMAP bridge. Prefer the
    // valid JMAP-role mailbox and repoint Gmail's wire-folder mapping to it.
    // All Mail and Starred are different: their Gmail roles are not JMAP
    // Mailbox roles, so retain their existing mailbox but strip the role.
    const ROLE_FOLDERS: &[(&str, &str)] = &[
        ("[Gmail]/Drafts", "drafts"),
        ("[Gmail]/Sent Mail", "sent"),
        ("[Gmail]/Spam", "junk"),
        ("[Gmail]/Trash", "trash"),
    ];
    for (folder, role) in ROLE_FOLDERS {
        let target: Option<i64> = conn
            .query_row(
                "SELECT id FROM mailboxes WHERE role = ?1 ORDER BY id LIMIT 1",
                params![role],
                |r| r.get(0),
            )
            .optional()?;
        let Some(target) = target else { continue };
        let mappings: Vec<(i64, i64)> = {
            let mut stmt = conn.prepare(
                "SELECT source_id, local_id FROM sync_id_imap
                 WHERE type_name = 'mailbox' AND folder = ?1",
            )?;
            stmt.query_map(params![folder], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for (source_id, old) in mappings {
            if old == target {
                continue;
            }
            conn.execute(
                "UPDATE sync_id_imap SET local_id = ?1
                 WHERE source_id = ?2 AND type_name = 'mailbox' AND folder = ?3",
                params![target, source_id, folder],
            )?;
            // Repoint every canonical Email membership that referenced the
            // duplicate mailbox before deleting it.
            replace_mailbox_membership(conn, old, target)?;
            if mailbox_is_unreferenced(conn, old)? {
                conn.execute("DELETE FROM mailboxes WHERE id = ?1", params![old])?;
                out.removed_mailboxes += 1;
            }
            out.canonical_mailboxes += 1;
        }
    }
    for folder in ["[Gmail]/All Mail", "[Gmail]/Starred"] {
        conn.execute(
            "UPDATE mailboxes SET role = NULL WHERE id IN (
                 SELECT local_id FROM sync_id_imap
                 WHERE type_name = 'mailbox' AND folder = ?1
             )",
            params![folder],
        )?;
    }
    Ok(())
}

fn replace_mailbox_membership(conn: &Connection, old: i64, new: i64) -> rusqlite::Result<()> {
    let rows: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, mailbox_ids FROM emails
             WHERE EXISTS (SELECT 1 FROM json_each(emails.mailbox_ids) WHERE value = ?1)",
        )?;
        stmt.query_map(params![old], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (email_id, raw) in rows {
        let mut ids = BTreeSet::new();
        for value in json_array(&raw)? {
            if let Some(id) = value.as_i64() {
                ids.insert(if id == old { new } else { id });
            }
        }
        let encoded = serde_json::to_string(&ids.into_iter().collect::<Vec<_>>())
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        conn.execute(
            "UPDATE emails SET mailbox_ids = ?1 WHERE id = ?2",
            params![encoded, email_id],
        )?;
    }
    Ok(())
}

fn mailbox_is_unreferenced(conn: &Connection, id: i64) -> rusqlite::Result<bool> {
    let mapped: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sync_id_imap WHERE type_name = 'mailbox' AND local_id = ?1)",
        params![id],
        |r| r.get(0),
    )?;
    let child: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mailboxes WHERE parent_id = ?1)",
        params![id],
        |r| r.get(0),
    )?;
    let email: bool = conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM emails, json_each(emails.mailbox_ids) WHERE json_each.value = ?1
         )",
        params![id],
        |r| r.get(0),
    )?;
    Ok(!mapped && !child && !email)
}

fn observed_mailboxes(conn: &Connection, email_id: i64) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT mb.local_id
         FROM sync_id_imap obs
         JOIN sync_id_imap mb
           ON mb.source_id = obs.source_id
          AND mb.type_name = 'mailbox'
          AND mb.folder = obs.folder
         WHERE obs.type_name = 'email' AND obs.local_id = ?1
         ORDER BY mb.local_id",
    )?;
    stmt.query_map(params![email_id], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()
}

fn json_array(raw: &str) -> rusqlite::Result<Vec<Value>> {
    serde_json::from_str::<Vec<Value>>(raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init;
    use crate::db::sources::{SourceKey, upsert_source};

    #[test]
    fn consolidation_preserves_observations_and_unions_state() {
        let mut c = Connection::open_in_memory().unwrap();
        init::apply_schema(&c).unwrap();
        let sid = upsert_source(
            &c,
            &SourceKey {
                kind: "imap".into(),
                session_url: "imaps://host".into(),
                account_id: "a".into(),
            },
            None,
            "a",
        )
        .unwrap();
        c.execute(
            "INSERT INTO mailboxes (id,name) VALUES (10,'INBOX'),(20,'All Mail')",
            [],
        )
        .unwrap();
        crate::db::imap_ids::insert_mailbox(&c, sid, "INBOX", 10).unwrap();
        crate::db::imap_ids::insert_mailbox(&c, sid, "[Gmail]/All Mail", 20).unwrap();
        let blob = crate::db::blobs::intern_blob(&c, b"same bytes").unwrap();
        c.execute(
            "INSERT INTO emails (id,blob_id,received_at,mailbox_ids,keywords,message_match)
             VALUES (100,?1,'2026-01-01Z','[10]','[\"$seen\"]','{}'),
                    (200,?1,'2026-01-01Z','[20]','[\"$flagged\"]','{}')",
            params![blob],
        )
        .unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "INBOX", 1, 1, 100, None).unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "[Gmail]/All Mail", 2, 2, 200, None).unwrap();

        let got = consolidate_by_blob(&mut c).unwrap();
        assert_eq!(got.groups, 1);
        assert_eq!(got.removed_rows, 1);
        assert_eq!(got.remapped_imap_observations, 1);
        assert_eq!(
            crate::db::imap_ids::email_observation_count(&c, sid, 100).unwrap(),
            2
        );
        let row: (String, String) = c
            .query_row(
                "SELECT mailbox_ids, keywords FROM emails WHERE id = 100",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, "[10,20]");
        assert_eq!(row.1, "[\"$flagged\",\"$seen\"]");
        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM emails", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
