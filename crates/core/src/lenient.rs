//! Files written by a newer version must still load: a value this version
//! can't read falls back to its default instead of making the file
//! unreadable (CONTRIBUTING.md).

use serde::de::DeserializeOwned;

/// Reads `table` as `T`, which must take a default for every missing field,
/// after leaving out each key whose value `T` can't read. Keys in `strict`
/// are kept, so an unreadable one is still an error.
pub(crate) fn from_table<T: DeserializeOwned>(
    mut table: toml::Table,
    strict: &[&str],
) -> Result<T, toml::de::Error> {
    let unreadable: Vec<String> = table
        .iter()
        .filter(|(key, _)| !strict.contains(&key.as_str()))
        .filter(|(key, value)| {
            let alone = toml::Table::from_iter([((*key).clone(), (*value).clone())]);
            toml::Value::Table(alone).try_into::<T>().is_err()
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in unreadable {
        tracing::warn!(key, "a saved value this version can't read is ignored");
        table.remove(&key);
    }
    toml::Value::Table(table).try_into()
}
