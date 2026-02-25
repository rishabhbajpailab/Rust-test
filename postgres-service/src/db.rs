//! PostgreSQL database layer.
//!
//! Uses [`sqlx`] with a generic `records` table for demonstration.
//! Real deployments should replace the raw-JSON approach with typed migrations
//! and domain-specific tables.

use anyhow::{Context, Result};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use uuid::Uuid;

/// Shared connection pool.
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// Connect to PostgreSQL using the supplied `database_url`.
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await
            .context("Failed to connect to PostgreSQL")?;

        Ok(Self { pool })
    }

    /// Run any pending migrations located in the `migrations/` directory next
    /// to the binary.  Creates the `records` table if it doesn't exist yet.
    pub async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS records (
                id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                table_name TEXT NOT NULL,
                payload    JSONB NOT NULL DEFAULT '{}',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create records table")?;

        Ok(())
    }

    // ------------------------------------------------------------------ //
    //  CRUD operations                                                     //
    // ------------------------------------------------------------------ //

    pub async fn create(&self, table_name: &str, payload: &str) -> Result<String> {
        let id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO records (table_name, payload)
            VALUES ($1, $2::jsonb)
            RETURNING id
            "#,
        )
        .bind(table_name)
        .bind(payload)
        .fetch_one(&self.pool)
        .await
        .context("INSERT failed")?;

        Ok(id.to_string())
    }

    pub async fn read(&self, id: &str, table_name: &str) -> Result<Option<DbRecord>> {
        let uuid = Uuid::parse_str(id).context("Invalid UUID")?;

        let row = sqlx::query(
            r#"
            SELECT id, table_name, payload::text, created_at::text, updated_at::text
            FROM records
            WHERE id = $1 AND table_name = $2
            "#,
        )
        .bind(uuid)
        .bind(table_name)
        .fetch_optional(&self.pool)
        .await
        .context("SELECT failed")?;

        Ok(row.map(|r| DbRecord {
            id: r.get::<Uuid, _>("id").to_string(),
            table_name: r.get("table_name"),
            payload: r.get("payload"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        }))
    }

    pub async fn list(
        &self,
        table_name: &str,
        _filter: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<DbRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, table_name, payload::text, created_at::text, updated_at::text
            FROM records
            WHERE table_name = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(table_name)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .context("LIST query failed")?;

        Ok(rows
            .into_iter()
            .map(|r| DbRecord {
                id: r.get::<Uuid, _>("id").to_string(),
                table_name: r.get("table_name"),
                payload: r.get("payload"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    pub async fn update(&self, id: &str, table_name: &str, payload: &str) -> Result<bool> {
        let uuid = Uuid::parse_str(id).context("Invalid UUID")?;

        let affected = sqlx::query(
            r#"
            UPDATE records
            SET payload    = $3::jsonb,
                updated_at = NOW()
            WHERE id = $1 AND table_name = $2
            "#,
        )
        .bind(uuid)
        .bind(table_name)
        .bind(payload)
        .execute(&self.pool)
        .await
        .context("UPDATE failed")?
        .rows_affected();

        Ok(affected > 0)
    }

    pub async fn delete(&self, id: &str, table_name: &str) -> Result<bool> {
        let uuid = Uuid::parse_str(id).context("Invalid UUID")?;

        let affected = sqlx::query(
            r#"DELETE FROM records WHERE id = $1 AND table_name = $2"#,
        )
        .bind(uuid)
        .bind(table_name)
        .execute(&self.pool)
        .await
        .context("DELETE failed")?
        .rows_affected();

        Ok(affected > 0)
    }
}

/// A row returned from the `records` table.
pub struct DbRecord {
    pub id: String,
    pub table_name: String,
    pub payload: String,
    pub created_at: String,
    pub updated_at: String,
}
