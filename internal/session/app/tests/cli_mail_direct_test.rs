mod common;

use common::{OrPanic as _, assert_success, run_session, stderr, stdout};

#[test]
fn direct_mail_cli_uses_session_identity_env() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());
    let sender_dir = daemon.dir.path().join("sender");
    let recipient_dir = daemon.dir.path().join("recipient");
    std::fs::create_dir_all(&sender_dir).or_panic("sender dir");
    std::fs::create_dir_all(&recipient_dir).or_panic("recipient dir");
    let sender_id = run_session(&daemon, "default", &sender_dir);
    let recipient_id = run_session(&daemon, "default", &recipient_dir);
    let thread = "direct-cli-session-identity";

    let sent = daemon
        .lilo_command()
        .env("LILO_AGENT_SESSION_ID", &sender_id)
        .args([
            "mail",
            "send",
            "--to",
            &format!("id:{recipient_id}"),
            "--content",
            "direct cli body",
            "--context-id",
            thread,
            "--intent",
            "inform",
        ])
        .output()
        .or_panic("lilo mail send executes");
    assert_success("lilo mail send", &sent);

    let peeked = daemon
        .lilo_command()
        .args(["mail", "peek", "--context-id", thread, "--output", "json"])
        .output()
        .or_panic("lilo mail peek executes");
    assert_success("lilo mail peek --output json", &peeked);
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&peeked)).or_panic("mail peek json parses");
    let sender = &json["response"]["messages"][0]["sender"];
    assert_eq!(sender["type"].as_str(), Some("session"));
    assert_eq!(sender["session_id"].as_str(), Some(sender_id.as_str()));

    let overview = daemon
        .lilo_command()
        .args(["mail", "peek"])
        .output()
        .or_panic("lilo mail peek overview executes");
    assert_success("lilo mail peek", &overview);
    let overview_stdout = stdout(&overview);
    assert!(overview_stdout.contains("CONTEXT"), "{overview_stdout}");
    assert!(overview_stdout.contains(thread), "{overview_stdout}");
    assert!(
        overview_stdout.contains("direct cli body"),
        "{overview_stdout}"
    );

    let read = daemon
        .lilo_command()
        .env("LILO_AGENT_SESSION_ID", &recipient_id)
        .args(["mail", "read"])
        .output()
        .or_panic("lilo mail read executes");
    assert_success("lilo mail read", &read);
    assert!(stdout(&read).contains("direct cli body"));

    let second_read = daemon
        .lilo_command()
        .env("LILO_AGENT_SESSION_ID", &recipient_id)
        .args(["mail", "read"])
        .output()
        .or_panic("second lilo mail read executes");
    assert_success("second lilo mail read", &second_read);
    assert!(stdout(&second_read).trim().is_empty());

    let agent_peek = daemon
        .lilo_command()
        .env("LILO_AGENT_SESSION_ID", &recipient_id)
        .args(["mail", "peek"])
        .output()
        .or_panic("agent lilo mail peek executes");
    assert!(!agent_peek.status.success());
    assert!(
        stderr(&agent_peek).contains("mail observation is operator-only"),
        "stderr:\n{}",
        stderr(&agent_peek)
    );
}
