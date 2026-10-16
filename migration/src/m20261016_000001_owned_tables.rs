//! The tables bmo-job owns, made in one step: each backend's schema is its own SQL text, so every type and
//! expression is spelled the way that backend reads it.

use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend};
use sea_orm_migration::{DbErr, MigrationName, MigrationTrait, SchemaManager};

pub struct Migration;

impl MigrationName for Migration {
  fn name(&self) -> &str {
    "m20261016_000001_owned_tables"
  }
}

/// The SQL that takes the schema up and the SQL that takes it down again, on one backend.
fn texts(backend: DbBackend) -> Result<(&'static str, &'static str), DbErr> {
  match backend {
    DbBackend::Postgres => Ok((
      include_str!("owned_tables/postgres.sql"),
      include_str!("owned_tables/postgres.down.sql"),
    )),
    DbBackend::Sqlite => {
      Ok((include_str!("owned_tables/sqlite.sql"), include_str!("owned_tables/sqlite.down.sql")))
    }
    other => Err(DbErr::Migration(format!("the service keeps no tables on {other:?}"))),
  }
}

#[async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let (up, _) = texts(manager.get_database_backend())?;

    manager.get_connection().execute_unprepared(up).await.map(drop)
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let (_, down) = texts(manager.get_database_backend())?;

    manager.get_connection().execute_unprepared(down).await.map(drop)
  }
}
