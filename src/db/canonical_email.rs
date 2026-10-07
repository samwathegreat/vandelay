/*
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::BTreeSet;

use rusqlite::{Connection, params};
use serde_json::Value;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Consolidation {
    pub groups: u64,
    pub removed_rows: u64,
    pub remapped_imap_observations: u64,
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
    let mut groups = Vec::new();
    {
        let mut stmt = tx.prepare(
            "SELECT blob_id FROM emails GROUP BY blob_id HAVING COUNT(*) > 1 ORDER BY blob_id",
        )?;
        groups = stmt
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
    }

    let mut out = Consolidation::default();
    for blob_id in groups {
        let mut ids = Vec::new();
        let mut mailboxes = BTreeSet::new();
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
                let (id, mailbox_json, keyword_json) = row?;
                ids.push(id);
                for value in json_array(&mailbox_json)? {
                    if let Some(id) = value.as_i64() {
                        mailboxes.insert(id);
                    }
                }
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

        let mailbox_json = serde_json::to_string(&mailboxes.into_iter().collect::<Vec<_>>())
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
        let blob = crate::db::blobs::intern_blob(&c, b"same bytes").unwrap();
        c.execute(
            "INSERT INTO emails (id,blob_id,received_at,mailbox_ids,keywords,message_match)
             VALUES (100,?1,'2026-01-01Z','[10]','[\"$seen\"]','{}'),
                    (200,?1,'2026-01-01Z','[20]','[\"$flagged\"]','{}')",
            params![blob],
        )
        .unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "INBOX", 1, 1, 100).unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "[Gmail]/All Mail", 2, 2, 200).unwrap();

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
