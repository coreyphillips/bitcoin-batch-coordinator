use anyhow::Result;
use sqlx::{Row, SqlitePool};
use super::schema::*;

/// Get all batches, optionally filtered by state
pub async fn get_batches(pool: &SqlitePool, state: Option<&str>) -> Result<Vec<Batch>> {
    let rows = if let Some(state) = state {
        sqlx::query(
            r#"
            SELECT id, intent_data, created_at, started_at, completed_at, state,
                   txid, total_fees, participant_count, raw_transaction
            FROM batches
            WHERE state = ?
            ORDER BY created_at DESC
            "#
        )
        .bind(state)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT id, intent_data, created_at, started_at, completed_at, state,
                   txid, total_fees, participant_count, raw_transaction
            FROM batches
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(pool)
        .await?
    };

    let batches = rows.iter().map(|row| Batch {
        id: row.get("id"),
        intent_data: row.get("intent_data"),
        created_at: row.get("created_at"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        state: row.get("state"),
        txid: row.get("txid"),
        total_fees: row.get("total_fees"),
        participant_count: row.get("participant_count"),
        raw_transaction: row.get("raw_transaction"),
    }).collect();

    Ok(batches)
}

/// Get a single batch by ID
pub async fn get_batch_by_id(pool: &SqlitePool, id: &str) -> Result<Option<Batch>> {
    let row = sqlx::query(
        r#"
        SELECT id, intent_data, created_at, started_at, completed_at, state,
               txid, total_fees, participant_count, raw_transaction
        FROM batches
        WHERE id = ?
        "#
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| Batch {
        id: r.get("id"),
        intent_data: r.get("intent_data"),
        created_at: r.get("created_at"),
        started_at: r.get("started_at"),
        completed_at: r.get("completed_at"),
        state: r.get("state"),
        txid: r.get("txid"),
        total_fees: r.get("total_fees"),
        participant_count: r.get("participant_count"),
        raw_transaction: r.get("raw_transaction"),
    }))
}

/// Get participants for a batch
pub async fn get_batch_participants(pool: &SqlitePool, batch_id: &str) -> Result<Vec<BatchParticipant>> {
    let rows = sqlx::query(
        r#"
        SELECT id, batch_id, pubkey, joined_at, commitment, reveal, signed
        FROM batch_participants
        WHERE batch_id = ?
        ORDER BY joined_at ASC
        "#
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    let participants = rows.iter().map(|row| BatchParticipant {
        id: row.get("id"),
        batch_id: row.get("batch_id"),
        pubkey: row.get("pubkey"),
        joined_at: row.get("joined_at"),
        commitment: row.get("commitment"),
        reveal: row.get("reveal"),
        signed: row.get("signed"),
    }).collect();

    Ok(participants)
}

/// Insert a new batch
pub async fn insert_batch(pool: &SqlitePool, batch: &Batch) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO batches (id, intent_data, created_at, started_at, completed_at, state,
                           txid, total_fees, participant_count, raw_transaction)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#
    )
    .bind(&batch.id)
    .bind(&batch.intent_data)
    .bind(batch.created_at)
    .bind(batch.started_at)
    .bind(batch.completed_at)
    .bind(&batch.state)
    .bind(&batch.txid)
    .bind(batch.total_fees)
    .bind(batch.participant_count)
    .bind(&batch.raw_transaction)
    .execute(pool)
    .await?;

    Ok(())
}

/// Update batch state
pub async fn update_batch_state(pool: &SqlitePool, id: &str, state: &str) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE batches
        SET state = ?
        WHERE id = ?
        "#
    )
    .bind(state)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get statistics
pub async fn get_stats(pool: &SqlitePool, days: i64) -> Result<Vec<Stats>> {
    let rows = sqlx::query(
        r#"
        SELECT date, batches_completed, total_participants, total_fees_saved
        FROM stats
        ORDER BY date DESC
        LIMIT ?
        "#
    )
    .bind(days)
    .fetch_all(pool)
    .await?;

    let stats = rows.iter().map(|row| Stats {
        date: row.get("date"),
        batches_completed: row.get("batches_completed"),
        total_participants: row.get("total_participants"),
        total_fees_saved: row.get("total_fees_saved"),
    }).collect();

    Ok(stats)
}

/// Get all bans
pub async fn get_bans(pool: &SqlitePool) -> Result<Vec<Ban>> {
    let now = chrono::Utc::now().timestamp();
    let rows = sqlx::query(
        r#"
        SELECT pubkey, banned_at, expires_at, offense_count, reason
        FROM bans
        WHERE expires_at IS NULL OR expires_at > ?
        ORDER BY banned_at DESC
        "#
    )
    .bind(now)
    .fetch_all(pool)
    .await?;

    let bans = rows.iter().map(|row| Ban {
        pubkey: row.get("pubkey"),
        banned_at: row.get("banned_at"),
        expires_at: row.get("expires_at"),
        offense_count: row.get("offense_count"),
        reason: row.get("reason"),
    }).collect();

    Ok(bans)
}

/// Get configuration value
pub async fn get_config(pool: &SqlitePool, key: &str) -> Result<Option<Config>> {
    let row = sqlx::query(
        r#"
        SELECT key, value, updated_at
        FROM config
        WHERE key = ?
        "#
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| Config {
        key: r.get("key"),
        value: r.get("value"),
        updated_at: r.get("updated_at"),
    }))
}

/// Set configuration value
pub async fn set_config(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO config (key, value, updated_at)
        VALUES (?, ?, ?)
        ON CONFLICT(key) DO UPDATE SET value = ?, updated_at = ?
        "#
    )
    .bind(key)
    .bind(value)
    .bind(now)
    .bind(value)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}
