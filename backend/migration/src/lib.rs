//! Keep SQLx validation strict while accepting a previously applied LF/CRLF variant.

use std::{borrow::Cow, path::Path};

use sha2::{Digest, Sha384};
use sqlx::{
    PgPool,
    migrate::{MigrateError, Migration, Migrator},
};

/// Existing Windows installations recorded CRLF checksums. Never rewrite their ledger.
/// Only an exact SHA-384 match of the same SQL with alternate line endings is accepted.
pub async fn run(pool: &PgPool, source: &Path) -> Result<(), MigrateError> {
    let mut migrator = Migrator::new(source).await?;
    let has_ledger: bool = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
        .fetch_one(pool)
        .await?;
    if has_ledger {
        let applied: Vec<(i64, Vec<u8>)> =
            sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations")
                .fetch_all(pool)
                .await?;
        for migration in migrator
            .migrations
            .to_mut()
            .iter_mut()
            .filter(|migration| migration.migration_type.is_up_migration())
        {
            if let Some((_, checksum)) = applied
                .iter()
                .find(|(version, _)| *version == migration.version)
            {
                accept_eol_variant(migration, checksum)?;
            }
        }
    }
    // SQLx still owns locking, dirty/missing-version checks and transactional execution.
    migrator.run(pool).await
}

fn accept_eol_variant(migration: &mut Migration, applied: &[u8]) -> Result<(), MigrateError> {
    if migration.checksum.as_ref() == applied {
        return Ok(());
    }
    let lf = migration.sql.replace("\r\n", "\n");
    let crlf = lf.replace('\n', "\r\n");
    for sql in [lf, crlf] {
        if Sha384::digest(sql.as_bytes()).as_slice() == applied {
            migration.sql = Cow::Owned(sql);
            migration.checksum = Cow::Owned(applied.to_vec());
            return Ok(());
        }
    }
    Err(MigrateError::VersionMismatch(migration.version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::migrate::MigrationType;

    fn migration(sql: &str) -> Migration {
        Migration::new(
            1,
            Cow::Borrowed("fixture"),
            MigrationType::Simple,
            Cow::Owned(sql.into()),
            false,
        )
    }

    #[test]
    fn exact_eol_variants_are_accepted_without_accepting_sql_changes() {
        for (source, stored) in [
            ("SELECT 1;\n", "SELECT 1;\r\n"),
            ("SELECT 1;\r\n", "SELECT 1;\n"),
        ] {
            let mut candidate = migration(source);
            let ledger = migration(stored);
            accept_eol_variant(&mut candidate, &ledger.checksum).unwrap();
            assert_eq!(candidate.sql, ledger.sql);
            assert_eq!(candidate.checksum, ledger.checksum);
            let mut changed = migration(&source.replace('1', "2"));
            assert!(matches!(
                accept_eol_variant(&mut changed, &ledger.checksum),
                Err(MigrateError::VersionMismatch(1))
            ));
            assert!(matches!(
                accept_eol_variant(&mut migration(source), &[0; 48]),
                Err(MigrateError::VersionMismatch(1))
            ));
        }
    }

    #[tokio::test]
    #[ignore = "requires dedicated admin_migration_test PostgreSQL database"]
    async fn historical_ledgers_new_install_and_sql_changes() {
        let url = std::env::var("ADMIN_MIGRATION_TEST_DATABASE_URL").expect("dedicated test URL");
        let admin = PgPool::connect(&url).await.unwrap();
        let database: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(&admin)
            .await
            .unwrap();
        assert_eq!(database, "admin_migration_test", "refuse working databases");
        for historical in [true, false] {
            let schema = format!("migration_test_{}", uuid::Uuid::new_v4().simple());
            sqlx::query(&format!("CREATE SCHEMA {schema}"))
                .execute(&admin)
                .await
                .unwrap();
            let path_schema = schema.clone();
            let pool = sqlx::postgres::PgPoolOptions::new()
                .after_connect(move |conn, _| {
                    let schema = path_schema.clone();
                    Box::pin(async move {
                        sqlx::query(&format!("SET search_path TO {schema}"))
                            .execute(conn)
                            .await?;
                        Ok(())
                    })
                })
                .connect(&url)
                .await
                .unwrap();
            let directory = tempfile::tempdir().unwrap();
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
            for entry in std::fs::read_dir(&source).unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name();
                let lf = std::fs::read_to_string(entry.path())
                    .unwrap()
                    .replace("\r\n", "\n");
                let contents = if historical
                    && ["0001", "0002", "0003", "0004", "0005", "0006", "0008"]
                        .iter()
                        .any(|prefix| name.to_string_lossy().starts_with(prefix))
                {
                    lf.replace('\n', "\r\n")
                } else {
                    lf
                };
                std::fs::write(directory.path().join(name), contents).unwrap();
            }
            if historical {
                Migrator::new(directory.path())
                    .await
                    .unwrap()
                    .run(&pool)
                    .await
                    .unwrap();
            } else {
                run(&pool, directory.path()).await.unwrap();
            }
            let before: Vec<(i64, Vec<u8>)> =
                sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
                    .fetch_all(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                before.iter().map(|row| row.0).collect::<Vec<_>>(),
                (1..=11).collect::<Vec<_>>()
            );
            // Test both historical CRLF -> LF and fresh LF -> CRLF without ledger writes.
            for entry in std::fs::read_dir(directory.path()).unwrap() {
                let path = entry.unwrap().path();
                let lf = std::fs::read_to_string(&path)
                    .unwrap()
                    .replace("\r\n", "\n");
                std::fs::write(
                    path,
                    if historical {
                        lf
                    } else {
                        lf.replace('\n', "\r\n")
                    },
                )
                .unwrap();
            }
            run(&pool, directory.path()).await.unwrap();
            let changed = directory.path().join("0001_admin_panel_v1.sql");
            let sql = std::fs::read_to_string(&changed).unwrap();
            std::fs::write(changed, format!("{sql}\nSELECT 1;\n")).unwrap();
            assert!(matches!(
                run(&pool, directory.path()).await,
                Err(MigrateError::VersionMismatch(1))
            ));
            let after: Vec<(i64, Vec<u8>)> =
                sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
                    .fetch_all(&pool)
                    .await
                    .unwrap();
            assert_eq!(before, after);
            pool.close().await;
            sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
                .execute(&admin)
                .await
                .unwrap();
        }
        admin.close().await;
    }
}
