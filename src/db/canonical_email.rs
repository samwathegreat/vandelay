/*
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GmailConsolidationAnalysis {
    pub observations: u64,
    pub populated: u64,
    pub missing: u64,
    pub gmail_identities: u64,
    pub duplicate_identity_groups: u64,
    pub rows_removed: u64,
    pub blob_conflict_groups: u64,
    pub keyword_conflict_groups: u64,
    pub max_rows_per_identity: u64,
    pub email_rows_before: u64,
    pub email_rows_after: u64,
}

impl GmailConsolidationAnalysis {
    pub fn safe_to_apply(&self) -> bool {
        self.missing == 0 && self.blob_conflict_groups == 0 && self.keyword_conflict_groups == 0
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GmailConsolidation {
    pub groups: u64,
    pub removed_rows: u64,
    pub remapped_imap_observations: u64,
    pub removed_export_mappings: u64,
    pub removed_takeout_mappings: u64,
    pub remapped_takeout_mappings: u64,
}

/// Analyze how Gmail X-GM-MSGID identity would canonicalize the archive.
///
/// Gmail identity is scoped by IMAP source. The report is read-only and treats
/// missing Gmail identity, differing RFC822 blobs, or differing keyword state
/// within one Gmail identity as blockers for an automatic apply.
pub fn analyze_gmail_identities(conn: &Connection) -> rusqlite::Result<GmailConsolidationAnalysis> {
    let observations: u64 = conn.query_row(
        "SELECT COUNT(*) FROM sync_id_imap WHERE type_name = 'email'",
        [],
        |r| r.get(0),
    )?;
    let populated: u64 = conn.query_row(
        "SELECT COUNT(*) FROM sync_id_imap
         WHERE type_name = 'email' AND gmail_msgid IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let gmail_identities: u64 = conn.query_row(
        "SELECT COUNT(*) FROM (
             SELECT source_id, gmail_msgid
             FROM sync_id_imap
             WHERE type_name = 'email' AND gmail_msgid IS NOT NULL
             GROUP BY source_id, gmail_msgid
         )",
        [],
        |r| r.get(0),
    )?;
    let email_rows_before: u64 = conn.query_row("SELECT COUNT(*) FROM emails", [], |r| r.get(0))?;

    let (duplicate_identity_groups, rows_removed, max_rows_per_identity): (u64, u64, u64) = conn
        .query_row(
            "WITH groups AS (
                 SELECT source_id, gmail_msgid, COUNT(DISTINCT local_id) AS n
                 FROM sync_id_imap
                 WHERE type_name = 'email' AND gmail_msgid IS NOT NULL
                 GROUP BY source_id, gmail_msgid
             )
             SELECT
                 COALESCE(SUM(CASE WHEN n > 1 THEN 1 ELSE 0 END), 0),
                 COALESCE(SUM(CASE WHEN n > 1 THEN n - 1 ELSE 0 END), 0),
                 COALESCE(MAX(n), 0)
             FROM groups",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

    let blob_conflict_groups: u64 = conn.query_row(
        "SELECT COUNT(*) FROM (
             SELECT obs.source_id, obs.gmail_msgid
             FROM sync_id_imap obs
             JOIN emails e ON e.id = obs.local_id
             WHERE obs.type_name = 'email' AND obs.gmail_msgid IS NOT NULL
             GROUP BY obs.source_id, obs.gmail_msgid
             HAVING COUNT(DISTINCT e.blob_id) > 1
         )",
        [],
        |r| r.get(0),
    )?;
    let keyword_conflict_groups: u64 = conn.query_row(
        "SELECT COUNT(*) FROM (
             SELECT obs.source_id, obs.gmail_msgid
             FROM sync_id_imap obs
             JOIN emails e ON e.id = obs.local_id
             WHERE obs.type_name = 'email' AND obs.gmail_msgid IS NOT NULL
             GROUP BY obs.source_id, obs.gmail_msgid
             HAVING COUNT(DISTINCT e.keywords) > 1
         )",
        [],
        |r| r.get(0),
    )?;

    let missing = observations.saturating_sub(populated);
    Ok(GmailConsolidationAnalysis {
        observations,
        populated,
        missing,
        gmail_identities,
        duplicate_identity_groups,
        rows_removed,
        blob_conflict_groups,
        keyword_conflict_groups,
        max_rows_per_identity,
        email_rows_before,
        email_rows_after: email_rows_before.saturating_sub(rows_removed),
    })
}

/// Collapse duplicate Email rows that share one authoritative Gmail
/// X-GM-MSGID. This refuses to run unless the analysis is conflict-free.
///
/// The lowest local Email id is retained. All IMAP observations are repointed
/// to it, mailbox membership is rebuilt from those observations, Takeout
/// mappings are preserved where possible, and JMAP export mappings for every
/// affected identity are discarded so a later export cannot inherit stale
/// duplicate target identity.
pub fn consolidate_by_gmail_identity(
    conn: &mut Connection,
) -> rusqlite::Result<GmailConsolidation> {
    let analysis = analyze_gmail_identities(conn)?;
    if !analysis.safe_to_apply() {
        return Err(rusqlite::Error::InvalidQuery);
    }

    let tx = conn.transaction()?;
    let groups: Vec<(i64, String)> = {
        let mut stmt = tx.prepare(
            "SELECT source_id, gmail_msgid
             FROM sync_id_imap
             WHERE type_name = 'email' AND gmail_msgid IS NOT NULL
             GROUP BY source_id, gmail_msgid
             HAVING COUNT(DISTINCT local_id) > 1
             ORDER BY source_id, gmail_msgid",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut out = GmailConsolidation::default();
    for (source_id, gmail_msgid) in groups {
        let ids: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT local_id
                 FROM sync_id_imap
                 WHERE source_id = ?1 AND type_name = 'email' AND gmail_msgid = ?2
                 ORDER BY local_id",
            )?;
            stmt.query_map(params![source_id, gmail_msgid], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let Some((&canonical, losers)) = ids.split_first() else {
            continue;
        };

        // No existing target mapping for a row in a formerly duplicated
        // identity is trustworthy after canonicalization.
        for id in &ids {
            out.removed_export_mappings += tx.execute(
                "DELETE FROM export_id_jmap WHERE type_name = 'email' AND local_id = ?1",
                params![id],
            )? as u64;
        }

        for loser in losers {
            out.remapped_imap_observations += tx.execute(
                "UPDATE sync_id_imap SET local_id = ?1
                 WHERE source_id = ?2 AND type_name = 'email'
                   AND gmail_msgid = ?3 AND local_id = ?4",
                params![canonical, source_id, gmail_msgid, loser],
            )? as u64;

            let takeout: Vec<(i64, String)> = {
                let mut stmt = tx.prepare(
                    "SELECT source_id, source_obj_id FROM sync_id_takeout
                     WHERE type_name = 'email' AND local_id = ?1",
                )?;
                stmt.query_map(params![loser], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?
            };
            for (takeout_source, source_obj_id) in takeout {
                let occupied: bool = tx.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM sync_id_takeout
                         WHERE source_id = ?1 AND type_name = 'email' AND local_id = ?2
                     )",
                    params![takeout_source, canonical],
                    |r| r.get(0),
                )?;
                if occupied {
                    out.removed_takeout_mappings += tx.execute(
                        "DELETE FROM sync_id_takeout
                         WHERE source_id = ?1 AND type_name = 'email' AND source_obj_id = ?2",
                        params![takeout_source, source_obj_id],
                    )? as u64;
                } else {
                    out.remapped_takeout_mappings += tx.execute(
                        "UPDATE sync_id_takeout SET local_id = ?1
                         WHERE source_id = ?2 AND type_name = 'email' AND source_obj_id = ?3",
                        params![canonical, takeout_source, source_obj_id],
                    )? as u64;
                }
            }

            tx.execute("DELETE FROM emails WHERE id = ?1", params![loser])?;
            out.removed_rows += 1;
        }

        let mailboxes = observed_mailboxes(&tx, canonical)?;
        let mailbox_json = serde_json::to_string(&mailboxes)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        tx.execute(
            "UPDATE emails SET mailbox_ids = ?1 WHERE id = ?2",
            params![mailbox_json, canonical],
        )?;
        out.groups += 1;
    }

    tx.commit()?;
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init;
    use crate::db::sources::{SourceKey, upsert_source};

    fn fixture() -> (Connection, i64) {
        let c = Connection::open_in_memory().unwrap();
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
        (c, sid)
    }

    #[test]
    fn analysis_reports_gmail_identity_plan() {
        let (c, sid) = fixture();
        let blob = crate::db::blobs::intern_blob(&c, b"same bytes").unwrap();
        c.execute(
            "INSERT INTO emails (id,blob_id,received_at,mailbox_ids,keywords,message_match)
             VALUES (100,?1,'2026-01-01Z','[10]','[\"$seen\"]','{}'),
                    (200,?1,'2026-01-01Z','[20]','[\"$seen\"]','{}')",
            params![blob],
        )
        .unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "INBOX", 1, 1, 100, Some(42)).unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "[Gmail]/All Mail", 2, 2, 200, Some(42))
            .unwrap();

        let got = analyze_gmail_identities(&c).unwrap();
        assert_eq!(got.observations, 2);
        assert_eq!(got.populated, 2);
        assert_eq!(got.missing, 0);
        assert_eq!(got.gmail_identities, 1);
        assert_eq!(got.duplicate_identity_groups, 1);
        assert_eq!(got.rows_removed, 1);
        assert_eq!(got.blob_conflict_groups, 0);
        assert_eq!(got.keyword_conflict_groups, 0);
        assert_eq!(got.email_rows_before, 2);
        assert_eq!(got.email_rows_after, 1);
        assert!(got.safe_to_apply());
    }

    #[test]
    fn analysis_blocks_different_blobs_for_same_gmail_identity() {
        let (c, sid) = fixture();
        let a = crate::db::blobs::intern_blob(&c, b"a").unwrap();
        let b = crate::db::blobs::intern_blob(&c, b"b").unwrap();
        c.execute(
            "INSERT INTO emails (id,blob_id,received_at,mailbox_ids,keywords,message_match)
             VALUES (100,?1,'2026-01-01Z','[10]','[]','{}'),
                    (200,?2,'2026-01-01Z','[20]','[]','{}')",
            params![a, b],
        )
        .unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "INBOX", 1, 1, 100, Some(42)).unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "[Gmail]/All Mail", 2, 2, 200, Some(42))
            .unwrap();

        let got = analyze_gmail_identities(&c).unwrap();
        assert_eq!(got.blob_conflict_groups, 1);
        assert!(!got.safe_to_apply());
    }

    #[test]
    fn consolidation_repoints_observations_and_rebuilds_membership() {
        let (mut c, sid) = fixture();
        let blob = crate::db::blobs::intern_blob(&c, b"same bytes").unwrap();
        c.execute(
            "INSERT INTO emails (id,blob_id,received_at,mailbox_ids,keywords,message_match)
             VALUES (100,?1,'2026-01-01Z','[10]','[\"$seen\"]','{}'),
                    (200,?1,'2026-01-01Z','[20]','[\"$seen\"]','{}')",
            params![blob],
        )
        .unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "INBOX", 1, 1, 100, Some(42)).unwrap();
        crate::db::imap_ids::insert_email(&c, sid, "[Gmail]/All Mail", 2, 2, 200, Some(42))
            .unwrap();

        let got = consolidate_by_gmail_identity(&mut c).unwrap();
        assert_eq!(got.groups, 1);
        assert_eq!(got.removed_rows, 1);
        assert_eq!(got.remapped_imap_observations, 1);
        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM emails", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let row: String = c
            .query_row("SELECT mailbox_ids FROM emails WHERE id = 100", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(row, "[10,20]");
        assert_eq!(
            crate::db::imap_ids::email_observation_count(&c, sid, 100).unwrap(),
            2
        );
    }
}
