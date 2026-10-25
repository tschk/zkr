use super::*;
use rusqlite::Transaction;

pub(crate) fn new_id(transaction: &Transaction<'_>) -> Result<String> {
    Ok(transaction.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?)
}

#[cfg(test)]
pub(crate) fn now_iso(transaction: &Transaction<'_>) -> Result<String> {
    Ok(
        transaction.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now')", [], |row| {
            row.get(0)
        })?,
    )
}
