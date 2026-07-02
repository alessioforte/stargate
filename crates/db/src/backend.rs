#[cfg(feature = "postgres")]
pub type Db = sqlx::Postgres;
#[cfg(feature = "sqlite")]
pub type Db = sqlx::Sqlite;

pub type Pool = sqlx::Pool<Db>;
pub type Tx<'a> = sqlx::Transaction<'a, Db>;

pub trait ReadExecutor<'c>: sqlx::Executor<'c, Database = Db> {}

impl<'c, T> ReadExecutor<'c> for T where T: sqlx::Executor<'c, Database = Db> {}

#[cfg(feature = "postgres")]
pub const LIKE: &str = "ILIKE";
#[cfg(feature = "sqlite")]
pub const LIKE: &str = "LIKE";

/// `ESCAPE` clause to append after every `LIKE`/`ILIKE $n` that binds a
/// pattern built with [`like_contains`], so user-supplied `%` and `_` are
/// matched literally rather than as wildcards.
pub const LIKE_ESCAPE: &str = "ESCAPE '\\'";

/// Build a `%…%` "contains" pattern for `LIKE`/`ILIKE`, escaping the wildcard
/// metacharacters (`\`, `%`, `_`) in `query` with a backslash. Must be used
/// with a clause that declares [`LIKE_ESCAPE`].
pub fn like_contains(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len() + 2);
    pattern.push('%');
    for ch in query.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

#[cfg(feature = "postgres")]
pub const MAX_BIND_PARAMETERS: usize = 65_535;
#[cfg(feature = "sqlite")]
pub const MAX_BIND_PARAMETERS: usize = 999;

#[cfg(test)]
mod tests {
    use super::like_contains;

    #[test]
    fn like_contains_escapes_wildcards() {
        assert_eq!(like_contains("abc"), "%abc%");
        assert_eq!(like_contains("50%"), "%50\\%%");
        assert_eq!(like_contains("a_b"), "%a\\_b%");
        assert_eq!(like_contains("a\\b"), "%a\\\\b%");
    }
}
