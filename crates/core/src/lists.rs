//! Ordered lists of track ids, stored by list id. The same track can appear
//! more than once. Items aren't tied to `tracks`, so rescans never change them.

use std::path::PathBuf;

use rusqlite::{Connection, Transaction, named_params};

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListId(pub i64);

impl ListId {
    /// Created by migration v12.
    pub(crate) const QUEUE: Self = Self(1);
}

/// Each item's track id, in order, with its path while the track is still in
/// the library (`None` once it has been deleted).
pub(crate) fn load_items(conn: &Connection, id: ListId) -> Result<Vec<(i64, Option<PathBuf>)>> {
    let mut stmt = conn.prepare(
        "SELECT i.track_id, t.path
         FROM list_items i LEFT JOIN tracks t ON t.id = i.track_id
         WHERE i.list_id = :list_id
         ORDER BY i.position",
    )?;
    let rows = stmt.query_map(named_params! { ":list_id": id.0 }, |row| {
        Ok((
            row.get::<_, i64>("track_id")?,
            row.get::<_, Option<String>>("path")?.map(PathBuf::from),
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Replaces the items of list `id`. Item `i` is saved at position `i`, which
/// the queue's cursor and shuffle order refer to.
pub(crate) fn save_items(tx: &Transaction, id: ListId, track_ids: &[i64]) -> Result<()> {
    tx.execute(
        "DELETE FROM list_items WHERE list_id = :list_id",
        named_params! { ":list_id": id.0 },
    )?;
    let mut insert = tx.prepare(
        "INSERT INTO list_items (list_id, position, track_id)
         VALUES (:list_id, :position, :track_id)",
    )?;
    for (position, track_id) in track_ids.iter().enumerate() {
        insert.execute(named_params! {
            ":list_id": id.0,
            ":position": position as i64,
            ":track_id": track_id,
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/lists_test.rs"]
mod tests;
