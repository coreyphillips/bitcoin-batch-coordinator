use anyhow::Result;
use sqlx::{Row, SqlitePool};
use super::schema::*;

/// Get the current coordinator identity
pub async fn get_coordinator_identity(pool: &SqlitePool) -> Result<Option<CoordinatorIdentity>> {
    let row = sqlx::query(
        r#"
        SELECT id, identity_type, encrypted_data, pubkey, created_at, is_active
        FROM coordinator_identity
        WHERE id = 1 AND is_active = TRUE
        "#
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| CoordinatorIdentity {
        id: r.get("id"),
        identity_type: r.get("identity_type"),
        encrypted_data: r.get("encrypted_data"),
        pubkey: r.get("pubkey"),
        created_at: r.get("created_at"),
        is_active: r.get("is_active"),
    }))
}

/// Save or update coordinator identity
pub async fn save_coordinator_identity(
    pool: &SqlitePool,
    identity_type: &str,
    encrypted_data: &[u8],
    pubkey: &str,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        r#"
        INSERT INTO coordinator_identity (id, identity_type, encrypted_data, pubkey, created_at, is_active)
        VALUES (1, ?, ?, ?, ?, TRUE)
        ON CONFLICT(id) DO UPDATE SET
            identity_type = excluded.identity_type,
            encrypted_data = excluded.encrypted_data,
            pubkey = excluded.pubkey,
            created_at = excluded.created_at,
            is_active = TRUE
        "#
    )
    .bind(identity_type)
    .bind(encrypted_data)
    .bind(pubkey)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

/// Delete coordinator identity
pub async fn delete_coordinator_identity(pool: &SqlitePool) -> Result<()> {
    sqlx::query("DELETE FROM coordinator_identity WHERE id = 1")
        .execute(pool)
        .await?;

    Ok(())
}
