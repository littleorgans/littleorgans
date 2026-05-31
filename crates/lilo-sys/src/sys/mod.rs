mod unsupported;

std::cfg_select! {
    target_family = "unix" => {
        mod unix;
        pub use unix::ProcessExitWatcher;
        pub(crate) use unix::{
            BlockingIpcStream, IpcListener, IpcStream, bind_ipc, connect_blocking_ipc,
            connect_ipc, current_uid, on_shutdown, peer_cred, pid_alive, remove_socket_file,
            send_signal, start_time_probe_for_pid, watch_process_exit,
        };
    }
    windows => {
        mod windows;
        pub use windows::ProcessExitWatcher;
        pub(crate) use windows::{
            BlockingIpcStream, IpcListener, IpcStream, bind_ipc, connect_blocking_ipc,
            connect_ipc, current_uid, on_shutdown, peer_cred, pid_alive, remove_socket_file,
            send_signal, start_time_probe_for_pid, watch_process_exit,
        };
    }
    _ => {
        pub use unsupported::ProcessExitWatcher;
        pub(crate) use unsupported::{
            BlockingIpcStream, IpcListener, IpcStream, bind_ipc, connect_blocking_ipc,
            connect_ipc, current_uid, on_shutdown, peer_cred, pid_alive, remove_socket_file,
            send_signal, start_time_probe_for_pid, watch_process_exit,
        };
    }
}
