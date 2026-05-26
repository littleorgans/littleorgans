#[tokio::test]
async fn session_id_conflict_includes_terminal_lifecycle() {
    let state = test_state().await;
    let session_id = Uuid::now_v7();
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

    assert_conflict(response, SpawnConflictKind::SessionId, session_id);
}

#[tokio::test]
async fn tmux_occupant_conflict_is_typed_without_force() {
    let state = test_state().await;
    let occupant = Uuid::now_v7();
    insert_running_tmux(&state, occupant, 60_000).await;

    let mut request = tmux_request(Uuid::now_v7(), false);
    let response = check(&state, &mut request)
        .await
        .expect("preflight")
        .expect("conflict");

    assert_conflict(response, SpawnConflictKind::TmuxPaneOccupancy, occupant);
}

#[tokio::test]
async fn force_kills_tmux_occupant_and_allows_spawn() {
    let state = test_state().await;
    let mut child = Command::new("sleep").arg("60").spawn().expect("sleep");
    let occupant = Uuid::now_v7();
    insert_running_tmux(&state, occupant, child.id()).await;

    let mut request = tmux_request(Uuid::now_v7(), true);
    let response = check(&state, &mut request)
        .await
        .expect("preflight");

    assert!(response.is_none(), "force should clear pane conflict");
    wait_for_child_exit(&mut child);
}
