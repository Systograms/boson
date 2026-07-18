//! PostgreSQL foundation: connection pool, migrations, and transactional outbox.

use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use boson_events::EventEnvelope;
use boson_kernel::{DatabaseConfig, DatabaseInspectionConfig};
use boson_ports::{
    CellKind, CellValue, ColumnSchema, DatabaseInspector, DatabaseInspectorCapabilities,
    DatabaseRow, ForeignKeySchema, PortError, RowCount, RowPage, RowQuery, TableRef, TableSchema,
    TableSummary,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerHeartbeat {
    pub name: String,
    pub last_heartbeat: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct LeasedEvent {
    pub envelope: EventEnvelope,
    pub attempts: u32,
}

impl Database {
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&config.url)
            .await?;
        Ok(Self { pool })
    }

    /// Applies migrations from `path` using the default `_sqlx_migrations` table.
    pub async fn migrate(&self, path: impl AsRef<Path>) -> Result<(), DatabaseError> {
        self.migrate_with_table(path, "_sqlx_migrations").await
    }

    /// Applies migrations compiled into the calling application.
    pub async fn migrate_embedded(
        &self,
        migrator: &sqlx::migrate::Migrator,
    ) -> Result<(), DatabaseError> {
        migrator.run(&self.pool).await?;
        Ok(())
    }

    /// Applies migrations tracked in a dedicated table.
    ///
    /// Capability-owned migration directories should use a unique table so they
    /// do not collide with platform migrations in `_sqlx_migrations`.
    pub async fn migrate_with_table(
        &self,
        path: impl AsRef<Path>,
        table_name: &str,
    ) -> Result<(), DatabaseError> {
        let mut migrator = sqlx::migrate::Migrator::new(path.as_ref()).await?;
        migrator.dangerous_set_table_name(table_name.to_owned());
        migrator.run(&self.pool).await?;
        Ok(())
    }

    pub async fn ping(&self) -> Result<(), DatabaseError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn publish(&self, event: &EventEnvelope) -> Result<(), DatabaseError> {
        sqlx::query(
            "INSERT INTO kernel.outbox
             (id, topic, payload, correlation_id, occurred_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(event.id)
        .bind(&event.topic)
        .bind(&event.payload)
        .bind(&event.correlation_id)
        .bind(event.occurred_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Publishes an outbox event inside an existing transaction.
    ///
    /// Capabilities should prefer this over raw `kernel.outbox` SQL so domain
    /// writes and event emission stay atomic.
    pub async fn publish_in_tx(
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        event: &EventEnvelope,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            "INSERT INTO kernel.outbox
             (id, topic, payload, correlation_id, occurred_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(event.id)
        .bind(&event.topic)
        .bind(&event.payload)
        .bind(&event.correlation_id)
        .bind(event.occurred_at)
        .execute(&mut **transaction)
        .await?;
        Ok(())
    }

    pub async fn heartbeat(&self, worker_name: &str) -> Result<(), DatabaseError> {
        sqlx::query(
            "INSERT INTO ops.worker_heartbeats (name, last_heartbeat)
             VALUES ($1, now())
             ON CONFLICT (name)
             DO UPDATE SET last_heartbeat = excluded.last_heartbeat",
        )
        .bind(worker_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn worker_heartbeats(&self) -> Result<Vec<WorkerHeartbeat>, DatabaseError> {
        let rows = sqlx::query(
            "SELECT name, last_heartbeat
             FROM ops.worker_heartbeats
             ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(WorkerHeartbeat {
                    name: row.try_get("name")?,
                    last_heartbeat: row.try_get("last_heartbeat")?,
                })
            })
            .collect()
    }

    pub async fn lease_events(
        &self,
        limit: usize,
        visibility: Duration,
        worker_id: &str,
    ) -> Result<Vec<LeasedEvent>, DatabaseError> {
        let mut transaction = self.pool.begin().await?;
        let rows = sqlx::query(
            "WITH candidates AS (
                SELECT id FROM kernel.outbox
                WHERE dispatched_at IS NULL AND run_at <= now()
                  AND (
                    status = 'pending'
                    OR (status = 'processing'
                        AND locked_at < now() - make_interval(secs => $2))
                  )
                ORDER BY run_at, created_at
                FOR UPDATE SKIP LOCKED
                LIMIT $1
             )
             UPDATE kernel.outbox AS events
             SET status = 'processing', locked_at = now(), locked_by = $3
             FROM candidates
             WHERE events.id = candidates.id
             RETURNING events.id, events.topic, events.payload,
                       events.correlation_id, events.occurred_at, events.attempts",
        )
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .bind(i64::try_from(visibility.as_secs()).unwrap_or(i64::MAX))
        .bind(worker_id)
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        rows.into_iter()
            .map(|row| {
                Ok(LeasedEvent {
                    envelope: EventEnvelope {
                        id: row.try_get("id")?,
                        topic: row.try_get("topic")?,
                        occurred_at: row.try_get("occurred_at")?,
                        correlation_id: row.try_get("correlation_id")?,
                        actor_id: None,
                        payload: row.try_get("payload")?,
                    },
                    attempts: u32::try_from(row.try_get::<i32, _>("attempts")?).unwrap_or(0),
                })
            })
            .collect()
    }

    pub async fn delivered_consumers(&self, event_id: Uuid) -> Result<Vec<String>, DatabaseError> {
        let rows = sqlx::query(
            "SELECT consumer FROM kernel.event_deliveries
             WHERE event_id = $1 AND status = 'succeeded'",
        )
        .bind(event_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("consumer").map_err(DatabaseError::from))
            .collect()
    }

    pub async fn record_delivery(
        &self,
        event_id: Uuid,
        consumer: &str,
        error: Option<&str>,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            "INSERT INTO kernel.event_deliveries
             (event_id, consumer, status, attempts, last_error,
              first_attempted_at, last_attempted_at, delivered_at)
             VALUES ($1, $2, CASE WHEN $3::TEXT IS NULL THEN 'succeeded' ELSE 'failed' END,
                     1, $3, now(), now(),
                     CASE WHEN $3::TEXT IS NULL THEN now() ELSE NULL END)
             ON CONFLICT (event_id, consumer) DO UPDATE
             SET status = excluded.status,
                 attempts = kernel.event_deliveries.attempts + 1,
                 last_error = excluded.last_error,
                 last_attempted_at = now(),
                 delivered_at = CASE WHEN excluded.status = 'succeeded'
                                     THEN now()
                                     ELSE kernel.event_deliveries.delivered_at END",
        )
        .bind(event_id)
        .bind(consumer)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn complete_event(
        &self,
        event_id: Uuid,
        worker_id: &str,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            "UPDATE kernel.outbox
             SET status = 'dispatched', dispatched_at = now(),
                 locked_at = NULL, locked_by = NULL, last_error = NULL
             WHERE id = $1 AND status = 'processing' AND locked_by = $2",
        )
        .bind(event_id)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn retry_event(
        &self,
        event_id: Uuid,
        worker_id: &str,
        error: &str,
        delay: Duration,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            "UPDATE kernel.outbox
             SET status = 'pending', attempts = attempts + 1,
                 run_at = now() + make_interval(secs => $4),
                 locked_at = NULL, locked_by = NULL, last_error = $3
             WHERE id = $1 AND status = 'processing' AND locked_by = $2",
        )
        .bind(event_id)
        .bind(worker_id)
        .bind(error)
        .bind(i64::try_from(delay.as_secs()).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[derive(Clone)]
pub struct PostgresInspector {
    pool: PgPool,
    allowed_namespaces: Arc<[String]>,
    redacted_columns: Arc<HashSet<String>>,
    statement_timeout_ms: u64,
    max_page_size: u32,
}

impl PostgresInspector {
    #[must_use]
    pub fn new(pool: PgPool, config: &DatabaseInspectionConfig) -> Self {
        Self {
            pool,
            allowed_namespaces: Arc::from(config.allowed_namespaces.clone()),
            redacted_columns: Arc::new(
                config
                    .redacted_columns
                    .iter()
                    .map(|column| column.to_lowercase())
                    .collect(),
            ),
            statement_timeout_ms: config.statement_timeout_ms.max(100),
            max_page_size: config.max_page_size.clamp(1, 100),
        }
    }

    fn namespace_allowed(&self, namespace: &str) -> bool {
        !is_system_namespace(namespace)
            && (self.allowed_namespaces.is_empty()
                || self
                    .allowed_namespaces
                    .iter()
                    .any(|allowed| allowed == namespace))
    }

    fn is_redacted(&self, column: &str) -> bool {
        self.redacted_columns.contains(&column.to_lowercase())
    }

    async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>, PortError> {
        let rows = sqlx::query(
            "SELECT attribute.attname AS column_name
             FROM pg_catalog.pg_constraint constraint_record
             JOIN pg_catalog.pg_class table_record
               ON table_record.oid = constraint_record.conrelid
             JOIN pg_catalog.pg_namespace namespace_record
               ON namespace_record.oid = table_record.relnamespace
             JOIN LATERAL unnest(constraint_record.conkey) WITH ORDINALITY
               AS key_column(attribute_number, position) ON true
             JOIN pg_catalog.pg_attribute attribute
               ON attribute.attrelid = table_record.oid
              AND attribute.attnum = key_column.attribute_number
             WHERE constraint_record.contype = 'p'
               AND namespace_record.nspname = $1
               AND table_record.relname = $2
             ORDER BY key_column.position",
        )
        .bind(&table.namespace)
        .bind(&table.name)
        .fetch_all(&self.pool)
        .await
        .map_err(port_unavailable)?;
        rows.into_iter()
            .map(|row| row.try_get("column_name").map_err(port_unavailable))
            .collect()
    }

    async fn columns(&self, table: &TableRef) -> Result<Vec<ColumnSchema>, PortError> {
        let primary_key = self.primary_key(table).await?;
        let rows = sqlx::query(
            "SELECT column_name,
                    CASE WHEN data_type = 'USER-DEFINED' THEN udt_name ELSE data_type END
                      AS data_type,
                    is_nullable = 'YES' AS nullable,
                    column_default
             FROM information_schema.columns
             WHERE table_schema = $1 AND table_name = $2
             ORDER BY ordinal_position",
        )
        .bind(&table.namespace)
        .bind(&table.name)
        .fetch_all(&self.pool)
        .await
        .map_err(port_unavailable)?;
        if rows.is_empty() {
            return Err(PortError::NotFound);
        }
        rows.into_iter()
            .map(|row| {
                let name: String = row.try_get("column_name").map_err(port_unavailable)?;
                let redacted = self.is_redacted(&name);
                Ok(ColumnSchema {
                    primary_key: primary_key.iter().any(|column| column == &name),
                    name,
                    data_type: row.try_get("data_type").map_err(port_unavailable)?,
                    nullable: row.try_get("nullable").map_err(port_unavailable)?,
                    redacted,
                    default: if redacted {
                        None
                    } else {
                        row.try_get("column_default").map_err(port_unavailable)?
                    },
                })
            })
            .collect()
    }

    async fn foreign_keys(&self, table: &TableRef) -> Result<Vec<ForeignKeySchema>, PortError> {
        let rows = sqlx::query(
            "SELECT constraint_record.conname AS constraint_name,
                    array_agg(source_attribute.attname ORDER BY key_column.position)
                      AS source_columns,
                    target_namespace.nspname AS target_namespace,
                    target_table.relname AS target_table,
                    array_agg(target_attribute.attname ORDER BY key_column.position)
                      AS target_columns
             FROM pg_catalog.pg_constraint constraint_record
             JOIN pg_catalog.pg_class source_table
               ON source_table.oid = constraint_record.conrelid
             JOIN pg_catalog.pg_namespace source_namespace
               ON source_namespace.oid = source_table.relnamespace
             JOIN pg_catalog.pg_class target_table
               ON target_table.oid = constraint_record.confrelid
             JOIN pg_catalog.pg_namespace target_namespace
               ON target_namespace.oid = target_table.relnamespace
             JOIN LATERAL unnest(constraint_record.conkey, constraint_record.confkey)
               WITH ORDINALITY AS key_column(source_number, target_number, position) ON true
             JOIN pg_catalog.pg_attribute source_attribute
               ON source_attribute.attrelid = source_table.oid
              AND source_attribute.attnum = key_column.source_number
             JOIN pg_catalog.pg_attribute target_attribute
               ON target_attribute.attrelid = target_table.oid
              AND target_attribute.attnum = key_column.target_number
             WHERE constraint_record.contype = 'f'
               AND source_namespace.nspname = $1
               AND source_table.relname = $2
             GROUP BY constraint_record.conname, target_namespace.nspname,
                      target_table.relname
             ORDER BY constraint_record.conname",
        )
        .bind(&table.namespace)
        .bind(&table.name)
        .fetch_all(&self.pool)
        .await
        .map_err(port_unavailable)?;
        rows.into_iter()
            .map(|row| {
                Ok(ForeignKeySchema {
                    name: row.try_get("constraint_name").map_err(port_unavailable)?,
                    columns: row.try_get("source_columns").map_err(port_unavailable)?,
                    referenced_table: TableRef {
                        namespace: row.try_get("target_namespace").map_err(port_unavailable)?,
                        name: row.try_get("target_table").map_err(port_unavailable)?,
                    },
                    referenced_columns: row.try_get("target_columns").map_err(port_unavailable)?,
                })
            })
            .collect()
    }

    async fn estimated_count(&self, table: &TableRef) -> Result<Option<RowCount>, PortError> {
        let value = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT greatest(table_record.reltuples::bigint, 0)
             FROM pg_catalog.pg_class table_record
             JOIN pg_catalog.pg_namespace namespace_record
               ON namespace_record.oid = table_record.relnamespace
             WHERE namespace_record.nspname = $1
               AND table_record.relname = $2
               AND table_record.relkind IN ('r', 'p')",
        )
        .bind(&table.namespace)
        .bind(&table.name)
        .fetch_optional(&self.pool)
        .await
        .map_err(port_unavailable)?
        .flatten();
        Ok(value.map(|count| RowCount {
            value: u64::try_from(count).unwrap_or(0),
            exact: false,
        }))
    }
}

#[async_trait]
impl DatabaseInspector for PostgresInspector {
    fn capabilities(&self) -> DatabaseInspectorCapabilities {
        DatabaseInspectorCapabilities {
            provider: "postgresql".into(),
            supports_namespaces: true,
            supports_exact_count: false,
            max_page_size: self.max_page_size,
        }
    }

    async fn list_tables(&self) -> Result<Vec<TableSummary>, PortError> {
        let rows = sqlx::query(
            "SELECT namespace_record.nspname AS namespace,
                    table_record.relname AS table_name,
                    greatest(table_record.reltuples::bigint, 0) AS estimated_rows
             FROM pg_catalog.pg_class table_record
             JOIN pg_catalog.pg_namespace namespace_record
               ON namespace_record.oid = table_record.relnamespace
             WHERE table_record.relkind IN ('r', 'p')
               AND namespace_record.nspname <> 'information_schema'
               AND namespace_record.nspname NOT LIKE 'pg_%'
               AND (cardinality($1::text[]) = 0
                    OR namespace_record.nspname = ANY($1))
             ORDER BY namespace_record.nspname, table_record.relname",
        )
        .bind(self.allowed_namespaces.as_ref())
        .fetch_all(&self.pool)
        .await
        .map_err(port_unavailable)?;

        let mut tables = Vec::with_capacity(rows.len());
        for row in rows {
            let table = TableRef {
                namespace: row.try_get("namespace").map_err(port_unavailable)?,
                name: row.try_get("table_name").map_err(port_unavailable)?,
            };
            if !self.namespace_allowed(&table.namespace) {
                continue;
            }
            let estimated_rows: i64 = row.try_get("estimated_rows").map_err(port_unavailable)?;
            tables.push(TableSummary {
                primary_key: self.primary_key(&table).await?,
                row_count: Some(RowCount {
                    value: u64::try_from(estimated_rows).unwrap_or(0),
                    exact: false,
                }),
                table,
            });
        }
        Ok(tables)
    }

    async fn describe_table(&self, table: &TableRef) -> Result<TableSchema, PortError> {
        if !self.namespace_allowed(&table.namespace) {
            return Err(PortError::NotFound);
        }
        let columns = self.columns(table).await?;
        let primary_key = columns
            .iter()
            .filter(|column| column.primary_key)
            .map(|column| column.name.clone())
            .collect();
        Ok(TableSchema {
            table: table.clone(),
            columns,
            primary_key,
            foreign_keys: self.foreign_keys(table).await?,
            row_count: self.estimated_count(table).await?,
        })
    }

    async fn query_rows(&self, table: &TableRef, request: RowQuery) -> Result<RowPage, PortError> {
        let schema = self.describe_table(table).await?;
        let limit = request.limit.clamp(1, self.max_page_size);
        let offset = request
            .cursor
            .as_deref()
            .map(str::parse::<u64>)
            .transpose()
            .map_err(|_| PortError::Invalid("invalid pagination cursor".into()))?
            .unwrap_or(0);

        for filter in &request.filters {
            let column = schema
                .columns
                .iter()
                .find(|column| column.name == filter.column)
                .ok_or_else(|| PortError::Invalid(format!("unknown column `{}`", filter.column)))?;
            if column.redacted {
                return Err(PortError::Invalid(format!(
                    "filtering redacted column `{}` is not allowed",
                    filter.column
                )));
            }
        }

        let select_list = schema
            .columns
            .iter()
            .map(|column| {
                let identifier = quote_identifier(&column.name);
                if column.redacted {
                    format!("NULL::text AS {identifier}")
                } else if column.data_type == "bytea" {
                    format!("encode({identifier}, 'base64') AS {identifier}")
                } else {
                    format!("{identifier}::text AS {identifier}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!(
            "SELECT {select_list} FROM {}.{}",
            quote_identifier(&table.namespace),
            quote_identifier(&table.name)
        );
        if !request.filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(
                &request
                    .filters
                    .iter()
                    .enumerate()
                    .map(|(index, filter)| {
                        format!(
                            "{}::text = ${}",
                            quote_identifier(&filter.column),
                            index + 1
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" AND "),
            );
        }
        sql.push_str(" ORDER BY ");
        if schema.primary_key.is_empty() {
            sql.push_str("ctid");
        } else {
            sql.push_str(
                &schema
                    .primary_key
                    .iter()
                    .map(|column| quote_identifier(column))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        let limit_parameter = request.filters.len() + 1;
        let offset_parameter = limit_parameter + 1;
        sql.push_str(&format!(
            " LIMIT ${limit_parameter} OFFSET ${offset_parameter}"
        ));

        let mut transaction = self.pool.begin().await.map_err(port_unavailable)?;
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(port_unavailable)?;
        sqlx::query("SELECT set_config('statement_timeout', $1, true)")
            .bind(self.statement_timeout_ms.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(port_unavailable)?;
        // Identifiers came from provider metadata and were quoted above; all
        // user-supplied filter values remain bind parameters.
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for filter in &request.filters {
            query = query.bind(&filter.value);
        }
        query = query
            .bind(i64::from(limit) + 1)
            .bind(i64::try_from(offset).unwrap_or(i64::MAX));
        let mut rows = query
            .fetch_all(&mut *transaction)
            .await
            .map_err(port_unavailable)?;
        transaction.commit().await.map_err(port_unavailable)?;

        let has_more = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        if has_more {
            rows.pop();
        }
        let mut result_rows = Vec::with_capacity(rows.len());
        for row in rows {
            let mut cells = BTreeMap::new();
            for column in &schema.columns {
                let value = if column.redacted {
                    CellValue {
                        kind: CellKind::Redacted,
                        value: None,
                    }
                } else {
                    let raw: Option<String> = row
                        .try_get(column.name.as_str())
                        .map_err(port_unavailable)?;
                    CellValue {
                        kind: raw
                            .as_ref()
                            .map_or(CellKind::Null, |_| cell_kind(&column.data_type)),
                        value: raw,
                    }
                };
                cells.insert(column.name.clone(), value);
            }
            result_rows.push(DatabaseRow { cells });
        }
        Ok(RowPage {
            table: table.clone(),
            columns: schema.columns,
            rows: result_rows,
            next_cursor: has_more.then(|| (offset + u64::from(limit)).to_string()),
        })
    }
}

fn is_system_namespace(namespace: &str) -> bool {
    namespace == "information_schema" || namespace.starts_with("pg_")
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn cell_kind(data_type: &str) -> CellKind {
    match data_type {
        "boolean" => CellKind::Boolean,
        "smallint" | "integer" | "bigint" | "decimal" | "numeric" | "real" | "double precision"
        | "smallserial" | "serial" | "bigserial" | "money" => CellKind::Number,
        "json" | "jsonb" => CellKind::Json,
        "bytea" => CellKind::Binary,
        "date"
        | "time without time zone"
        | "time with time zone"
        | "timestamp without time zone"
        | "timestamp with time zone"
        | "interval" => CellKind::DateTime,
        "text" | "character varying" | "character" | "uuid" | "citext" => CellKind::Text,
        _ => CellKind::Other,
    }
}

fn port_unavailable(error: impl std::fmt::Display) -> PortError {
    PortError::Unavailable(error.to_string())
}

#[cfg(test)]
mod inspection_tests {
    use super::*;

    #[test]
    fn quotes_provider_identifiers() {
        assert_eq!(quote_identifier("normal"), "\"normal\"");
        assert_eq!(quote_identifier("odd\"name"), "\"odd\"\"name\"");
    }

    #[test]
    fn system_namespaces_are_hidden() {
        assert!(is_system_namespace("pg_catalog"));
        assert!(is_system_namespace("information_schema"));
        assert!(!is_system_namespace("public"));
    }

    #[test]
    fn preserves_number_kind_for_large_values() {
        assert!(matches!(cell_kind("bigint"), CellKind::Number));
        assert!(matches!(cell_kind("numeric"), CellKind::Number));
    }
}
