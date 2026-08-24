#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn session_id_conflict_includes_terminal_lifecycle() {
    let (state, testdb) = test_state().await;
    let session_id = SessionId::from_uuid(uuid::Uuid::now_v7());
    let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
    state
        .store()
        .insert_forking(&lifecycle)
        .await
        .expect("insert");
    lifecycle.mark_lost(lilo_rm_core::LostEvidence::PidNotAlive);
    state
        .store()
        .update_lifecycle(&lifecycle)
        .await
        .expect("terminal");

    let mut request = headless_request(session_id, false);
    let response = check(&state, &mut request)
        .await
        .expect("preflight")
        .expect("conflict");

    assert_conflict(&response, SpawnConflictKind::SessionId, session_id);
    testdb.cleanup().await.expect("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn tmux_occupant_conflict_is_typed_without_force() {
    let (state, testdb) = test_state().await;
    let occupant = SessionId::from_uuid(uuid::Uuid::now_v7());
    insert_running_tmux(&state, occupant, 60_000, Utc::now()).await;

    let mut request = tmux_request(SessionId::from_uuid(uuid::Uuid::now_v7()), false);
    let response = check(&state, &mut request)
        .await
        .expect("preflight")
        .expect("conflict");

    assert_conflict(&response, SpawnConflictKind::TmuxPaneOccupancy, occupant);
    testdb.cleanup().await.expect("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn force_kills_tmux_occupant_and_allows_spawn() {
    let (state, testdb) = test_state().await;
    let mut child = ChildGuard::spawn();
    let occupant = SessionId::from_uuid(uuid::Uuid::now_v7());
    insert_running_tmux(&state, occupant, child.id(), child.start_time()).await;

    let mut request = tmux_request(SessionId::from_uuid(uuid::Uuid::now_v7()), true);
    let response = check(&state, &mut request)
        .await
        .expect("preflight");

    assert!(response.is_none(), "force should clear pane conflict");
    child.wait_for_exit();
    testdb.cleanup().await.expect("test db cleans up");
}
