1. Add `pub(crate) fn sql_json_error(error: serde_json::Error) -> rusqlite::Error` to `src/store.rs`.
2. Remove `json_error` from `src/store/summaries.rs` and update references to use `crate::store::sql_json_error` or just `super::sql_json_error`.
3. Remove `sql_json_error` from `src/store/export.rs` and update references (actually `export.rs` already uses `sql_json_error`, so we just import it or rely on it being in `super`).
4. Update `lifecycle.rs` to use it if applicable (or just fix `summaries.rs` and `export.rs`).
5. Run `cargo fmt`, `cargo clippy`, and tests.
