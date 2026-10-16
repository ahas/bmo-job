//! The schema of the tables bmo-job owns, as migrations that run up and down (docs/schema.md).

use sea_orm_migration::MigrationTrait;
pub use sea_orm_migration::MigratorTrait;

mod m20261016_000001_owned_tables;

/// Every migration of the service, in the order they run.
pub struct Migrator;

impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(m20261016_000001_owned_tables::Migration)]
  }
}
