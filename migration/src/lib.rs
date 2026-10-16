//! The schema of the tables bmo-job owns, as migrations that run up and down (docs/schema.md). Each
//! migration is SQL text of its own on each backend, so every type and expression is spelled the way that
//! backend reads it.

pub use sea_orm_migration::MigratorTrait;
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend};
use sea_orm_migration::{DbErr, MigrationName, MigrationTrait, SchemaManager};

/// Every migration of the service, in the order they run.
pub struct Migrator;

impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(Sql {
        name: "m20261016_000001_owned_tables",
        postgres: (
          include_str!("owned_tables/postgres.sql"),
          include_str!("owned_tables/postgres.down.sql"),
        ),
        sqlite: (include_str!("owned_tables/sqlite.sql"), include_str!("owned_tables/sqlite.down.sql")),
      }),
      Box::new(Sql {
        name: "m20261017_000001_todo_talk",
        postgres: (include_str!("todo_talk/postgres.sql"), include_str!("todo_talk/postgres.down.sql")),
        sqlite: (include_str!("todo_talk/sqlite.sql"), include_str!("todo_talk/sqlite.down.sql")),
      }),
    ]
  }
}

/// One migration: the SQL that takes it up and the SQL that takes it down again, on each backend.
struct Sql {
  name: &'static str,
  postgres: (&'static str, &'static str),
  sqlite: (&'static str, &'static str),
}

impl Sql {
  fn texts(&self, backend: DbBackend) -> Result<(&'static str, &'static str), DbErr> {
    match backend {
      DbBackend::Postgres => Ok(self.postgres),
      DbBackend::Sqlite => Ok(self.sqlite),
      other => Err(DbErr::Migration(format!("the service keeps no tables on {other:?}"))),
    }
  }
}

impl MigrationName for Sql {
  fn name(&self) -> &str {
    self.name
  }
}

#[async_trait]
impl MigrationTrait for Sql {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let (up, _) = self.texts(manager.get_database_backend())?;

    manager.get_connection().execute_unprepared(up).await.map(drop)
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let (_, down) = self.texts(manager.get_database_backend())?;

    manager.get_connection().execute_unprepared(down).await.map(drop)
  }
}
