use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use uuid::Uuid;

pub struct TmuxSession {
    name: String,
    server_label: String,
}

impl TmuxSession {
    pub fn start(prefix: &str) -> Option<Self> {
        if !available() {
            return None;
        }
        let id = Uuid::now_v7().simple();
        let name = format!("{prefix}-{id}");
        let server_label = name.clone();
        let session = Self { name, server_label };
        session.tmux(["new-session", "-d", "-s", session.name()]);
        Some(session)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn server_label(&self) -> &str {
        &self.server_label
    }

    pub fn pane(&self) -> String {
        self.tmux_stdout(["list-panes", "-t", &self.name, "-F", "#S:#I.#P"])
            .lines()
            .next()
            .expect("pane")
            .to_owned()
    }

    pub fn assert_pane_listed(&self, pane: &str) {
        let panes = self.tmux_stdout(["list-panes", "-s", "-t", &self.name, "-F", "#S:#I.#P"]);
        assert!(panes.lines().any(|line| line == pane), "{panes}");
    }

    pub fn pane_alive(&self, pane: &str) -> bool {
        let output = self.run_tmux(["list-panes", "-s", "-t", &self.name, "-F", "#S:#I.#P"]);
        output.status.success() && stdout(output).lines().any(|line| line == pane)
    }

    pub fn wait_for_capture(&self, needle: &str) {
        let timeout = Duration::from_secs(5);
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.capture().contains(needle) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("tmux pane never contained {needle}");
    }

    pub fn capture(&self) -> String {
        self.tmux_stdout(["capture-pane", "-p", "-t", &self.name])
    }

    pub fn kill(&self) {
        let _ = self.run_tmux(["kill-session", "-t", &self.name]);
    }

    pub fn resize_height(&self, rows: u32) {
        self.tmux(["resize-pane", "-t", &self.name, "-y", &rows.to_string()]);
    }

    pub fn send_ctrl_c(&self, pane: &str) -> bool {
        self.run_tmux(["send-keys", "-t", pane, "C-c"])
            .status
            .success()
    }

    fn tmux<const N: usize>(&self, args: [&str; N]) {
        let output = self.run_tmux(args);
        assert!(output.status.success(), "tmux command failed: {output:?}");
    }

    fn tmux_stdout<const N: usize>(&self, args: [&str; N]) -> String {
        let output = self.run_tmux(args);
        assert!(output.status.success(), "tmux command failed: {output:?}");
        stdout(output)
    }

    fn run_tmux<const N: usize>(&self, args: [&str; N]) -> Output {
        run_tmux_on(&self.server_label, args)
    }
}

impl Drop for TmuxSession {
    fn drop(&mut self) {
        let _ = self.run_tmux(["kill-server"]);
    }
}

pub fn available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_tmux_on<const N: usize>(server_label: &str, args: [&str; N]) -> Output {
    Command::new("tmux")
        .arg("-L")
        .arg(server_label)
        .args(args)
        .output()
        .expect("tmux")
}

fn stdout(output: Output) -> String {
    String::from_utf8(output.stdout).expect("tmux stdout")
}
