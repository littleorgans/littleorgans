use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use uuid::Uuid;

pub struct TmuxSession {
    name: String,
}

impl TmuxSession {
    pub fn start(prefix: &str) -> Option<Self> {
        if !available() {
            return None;
        }
        let name = format!("{prefix}-{}", Uuid::now_v7().simple());
        tmux(["new-session", "-d", "-s", &name]);
        Some(Self { name })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn pane(&self) -> String {
        tmux_stdout(["list-panes", "-t", &self.name, "-F", "#S:#I.#P"])
            .lines()
            .next()
            .expect("pane")
            .to_owned()
    }

    pub fn assert_pane_listed(&self, pane: &str) {
        let panes = tmux_stdout(["list-panes", "-s", "-t", &self.name, "-F", "#S:#I.#P"]);
        assert!(panes.lines().any(|line| line == pane), "{panes}");
    }

    pub fn pane_alive(&self, pane: &str) -> bool {
        let output = run_tmux(["list-panes", "-s", "-t", &self.name, "-F", "#S:#I.#P"]);
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
        tmux_stdout(["capture-pane", "-p", "-t", &self.name])
    }

    pub fn kill(&self) {
        let _ = run_tmux(["kill-session", "-t", &self.name]);
    }

    pub fn resize_height(&self, rows: u32) {
        tmux(["resize-pane", "-t", &self.name, "-y", &rows.to_string()]);
    }

    pub fn send_ctrl_c(&self, pane: &str) -> bool {
        run_tmux(["send-keys", "-t", pane, "C-c"]).status.success()
    }
}

impl Drop for TmuxSession {
    fn drop(&mut self) {
        self.kill();
    }
}

pub fn available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn tmux<const N: usize>(args: [&str; N]) {
    let output = run_tmux(args);
    assert!(output.status.success(), "tmux command failed: {output:?}");
}

fn tmux_stdout<const N: usize>(args: [&str; N]) -> String {
    let output = run_tmux(args);
    assert!(output.status.success(), "tmux command failed: {output:?}");
    stdout(output)
}

fn run_tmux<const N: usize>(args: [&str; N]) -> Output {
    Command::new("tmux").args(args).output().expect("tmux")
}

fn stdout(output: Output) -> String {
    String::from_utf8(output.stdout).expect("tmux stdout")
}
