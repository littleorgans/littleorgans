mod unsupported;

std::cfg_select! {
    target_family = "unix" => {
        mod unix;
        pub use unix::ProcessExitWatcher;
        pub(crate) use unix::{
            current_uid, peer_cred, pid_alive, send_signal, start_time_probe_for_pid,
            watch_process_exit,
        };
    }
    windows => {
        mod windows;
        pub use windows::ProcessExitWatcher;
        pub(crate) use windows::{
            current_uid, peer_cred, pid_alive, send_signal, start_time_probe_for_pid,
            watch_process_exit,
        };
    }
    _ => {
        pub use unsupported::ProcessExitWatcher;
        pub(crate) use unsupported::{
            current_uid, peer_cred, pid_alive, send_signal, start_time_probe_for_pid,
            watch_process_exit,
        };
    }
}
