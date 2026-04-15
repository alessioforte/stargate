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

#[cfg(feature = "postgres")]
pub const MAX_BIND_PARAMETERS: usize = 65_535;
#[cfg(feature = "sqlite")]
pub const MAX_BIND_PARAMETERS: usize = 999;
