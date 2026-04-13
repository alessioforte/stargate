#[cfg(feature = "postgres")]
pub type Db = sqlx::Postgres;
#[cfg(feature = "sqlite")]
pub type Db = sqlx::Sqlite;

pub type Pool = sqlx::Pool<Db>;
pub type Tx<'a> = sqlx::Transaction<'a, Db>;

#[cfg(feature = "postgres")]
pub const LIKE: &str = "ILIKE";
#[cfg(feature = "sqlite")]
pub const LIKE: &str = "LIKE";
