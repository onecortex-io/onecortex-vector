use uuid::Uuid;

#[tokio::test]
async fn pool_connects_and_migrates() {
    dotenvy::dotenv().ok();
    let config = onecortex_vector::config::AppConfig::from_env().unwrap();
    let pool = onecortex_vector::db::pool::create_pool(&config)
        .await
        .unwrap();

    // Verify catalog schema exists
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = '_onecortex_vector')"
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        row.0,
        "_onecortex_vector schema should exist after migrations"
    );
}

#[tokio::test]
async fn lifecycle_create_and_drop() {
    dotenvy::dotenv().ok();
    let config = onecortex_vector::config::AppConfig::from_env().unwrap();
    let pool = onecortex_vector::db::pool::create_pool(&config)
        .await
        .unwrap();

    let collection_id = uuid::Uuid::new_v4();
    let table_name = onecortex_vector::db::lifecycle::table_name_for(collection_id);

    // First: insert a row into _onecortex_vector.collections so the FK and status update work
    sqlx::query(
        "INSERT INTO _onecortex_vector.collections (id, name, dimension, metric) VALUES ($1, $2, $3, $4)"
    )
    .bind(collection_id)
    .bind(format!("test-{}", collection_id.simple()))
    .bind(3_i32)
    .bind("cosine")
    .execute(&pool)
    .await
    .unwrap();

    // Create the table
    onecortex_vector::db::lifecycle::create_collection_table(
        &pool,
        collection_id,
        3,
        "cosine",
        50,
        100,
        false,
    )
    .await
    .unwrap();

    // Verify table exists in _onecortex_vector schema
    let (exists,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema = '_onecortex' AND table_name = $1)",
    )
    .bind(&table_name)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        exists,
        "Table should exist in _onecortex_vector after create"
    );

    // Verify status is 'ready'
    let (status,): (String,) =
        sqlx::query_as("SELECT status FROM _onecortex_vector.collections WHERE id = $1")
            .bind(collection_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "ready");

    // Drop the table
    onecortex_vector::db::lifecycle::drop_collection_table(&pool, collection_id)
        .await
        .unwrap();

    // Verify table is gone
    let (exists_after,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema = '_onecortex' AND table_name = $1)",
    )
    .bind(&table_name)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!exists_after, "Table should be gone after drop");

    // Verify collection row is deleted
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM _onecortex_vector.collections WHERE id = $1")
            .bind(collection_id)
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert!(row.is_none(), "Collection row should be deleted after drop");
}
