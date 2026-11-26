//! Ordered lists of track ids, stored by list id. The same track can appear
//! more than once. Items aren't tied to `tracks`, so rescans never change them.

use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use rusqlite::{Connection, Transaction, named_params};
use serde::{Deserialize, Serialize};

use crate::{Result, normalize};

/// Shown and parsed as its number, which is how QML names a playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ListId(i64);

impl fmt::Display for ListId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ListId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub id: ListId,
    pub name: String,
    /// In order, and kept when a track leaves the library.
    pub track_ids: Vec<i64>,
}

impl Playlist {
    /// `playlists` ordered by name, ignoring case, then by id.
    pub fn by_name(playlists: &[Self]) -> Vec<&Self> {
        let mut sorted: Vec<(String, &Self)> = playlists
            .iter()
            .map(|p| (normalize::key(&p.name), p))
            .collect();
        sorted.sort_by(|(a, p), (b, q)| a.cmp(b).then(p.id.0.cmp(&q.id.0)));
        sorted.into_iter().map(|(_, p)| p).collect()
    }

    /// An id that no list in `playlists` has, nor the queue.
    pub fn new_id(playlists: &[Self]) -> ListId {
        let largest = playlists
            .iter()
            .map(|p| p.id.0)
            .fold(ListId::QUEUE.0, i64::max);
        ListId(largest.saturating_add(1))
    }

    /// `id` while it is one of `playlists`, else the first of them by name.
    pub fn existing_or_first(id: Option<ListId>, playlists: &[Self]) -> Option<ListId> {
        match id {
            Some(id) if playlists.iter().any(|p| p.id == id) => Some(id),
            _ => Self::by_name(playlists).first().map(|p| p.id),
        }
    }
}

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

/// Every playlist, oldest first.
pub(crate) fn load_playlists(conn: &Connection) -> Result<Vec<Playlist>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.name, i.track_id
         FROM lists l LEFT JOIN list_items i ON i.list_id = l.id
         WHERE l.kind = 'playlist'
         ORDER BY l.created_ns, l.id, i.position",
    )?;
    let mut rows = stmt.query([])?;
    let mut playlists: Vec<Playlist> = Vec::new();
    while let Some(row) = rows.next()? {
        let id = ListId(row.get("id")?);
        if playlists.last().is_none_or(|last| last.id != id) {
            playlists.push(Playlist {
                id,
                name: row.get::<_, Option<String>>("name")?.unwrap_or_default(),
                track_ids: Vec::new(),
            });
        }
        if let (Some(playlist), Some(track_id)) =
            (playlists.last_mut(), row.get::<_, Option<i64>>("track_id")?)
        {
            playlist.track_ids.push(track_id);
        }
    }
    Ok(playlists)
}

/// Replaces the items of list `id`. Item `i` is saved at position `i`, which
/// the queue's cursor refers to.
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

pub(crate) fn save_playlists(tx: &Transaction, playlists: &[Playlist]) -> Result<()> {
    let kept: HashSet<ListId> = playlists.iter().map(|p| p.id).collect();
    let stored: Vec<i64> = tx
        .prepare("SELECT id FROM lists WHERE kind = 'playlist'")?
        .query_map([], |row| row.get("id"))?
        .collect::<rusqlite::Result<_>>()?;
    for id in stored.into_iter().map(ListId) {
        if !kept.contains(&id) {
            tx.execute(
                "DELETE FROM lists WHERE id = :id",
                named_params! { ":id": id.0 },
            )?;
        }
    }
    let mut upsert = tx.prepare(
        "INSERT INTO lists (id, kind, name, created_ns)
         VALUES (:id, 'playlist', :name, CAST(unixepoch('subsec') * 1000 AS INTEGER) * 1000000)
         ON CONFLICT (id) DO UPDATE SET name = excluded.name WHERE kind = 'playlist'",
    )?;
    for playlist in playlists {
        let saved = upsert.execute(named_params! {
            ":id": playlist.id.0,
            ":name": playlist.name,
        })?;
        if saved > 0 {
            save_items(tx, playlist.id, &playlist.track_ids)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/lists_test.rs"]
mod tests;
