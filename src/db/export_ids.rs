/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

use crate::types::ObjectType;

pub fn insert(
    conn: &Connection,
    account_id: &str,
    ty: ObjectType,
    local_id: i64,
    jmap_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO export_id_jmap (account_id, type_name, local_id, jmap_id)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(account_id, type_name, local_id)
         DO UPDATE SET jmap_id = excluded.jmap_id",
        params![account_id, ty.jmap_name(), local_id, jmap_id],
    )?;
    Ok(())
}

pub fn target_for(
    conn: &Connection,
    account_id: &str,
    ty: ObjectType,
    local_id: i64,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT jmap_id FROM export_id_jmap
         WHERE account_id = ?1 AND type_name = ?2 AND local_id = ?3",
        params![account_id, ty.jmap_name(), local_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn local_to_target(
    conn: &Connection,
    account_id: &str,
    ty: ObjectType,
) -> Result<HashMap<i64, String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT local_id, jmap_id FROM export_id_jmap
         WHERE account_id = ?1 AND type_name = ?2",
    )?;
    let rows = stmt.query_map(params![account_id, ty.jmap_name()], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (local, target) = row?;
        out.insert(local, target);
    }
    Ok(out)
}

pub fn delete_local(
    conn: &Connection,
    account_id: &str,
    ty: ObjectType,
    local_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM export_id_jmap
         WHERE account_id = ?1 AND type_name = ?2 AND local_id = ?3",
        params![account_id, ty.jmap_name(), local_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init;

    #[test]
    fn export_mapping_roundtrip_and_replace() {
        let c = Connection::open_in_memory().unwrap();
        init::apply_schema(&c).unwrap();

        insert(&c, "account-a", ObjectType::Email, 42, "E1").unwrap();
        assert_eq!(
            target_for(&c, "account-a", ObjectType::Email, 42).unwrap(),
            Some("E1".to_owned())
        );

        insert(&c, "account-a", ObjectType::Email, 42, "E2").unwrap();
        assert_eq!(
            target_for(&c, "account-a", ObjectType::Email, 42).unwrap(),
            Some("E2".to_owned())
        );
        assert_eq!(
            target_for(&c, "account-b", ObjectType::Email, 42).unwrap(),
            None
        );
    }
}
