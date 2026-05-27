use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct ToolExample {
    pub tool: &'static str,
    pub arguments: Value,
}

pub fn examples() -> Vec<ToolExample> {
    vec![
        ToolExample {
            tool: "session_run",
            arguments: json!({
                "runtime": "claude",
                "role": "engineer",
                "dir": "/Users/you/code/session-matters",
                "namespace": "project-alpha",
                "target": "headless",
                "labels": ["area=auth", "pri=high"]
            }),
        },
        ToolExample {
            tool: "session_list",
            arguments: json!({
                "selector": "namespace:project-alpha"
            }),
        },
        ToolExample {
            tool: "session_get",
            arguments: json!({
                "id": "019e32e3-0000-7000-8000-000000000000"
            }),
        },
        ToolExample {
            tool: "session_capture",
            arguments: json!({
                "id": "019e32e3-0000-7000-8000-000000000000",
                "scrollback_lines": 500
            }),
        },
        ToolExample {
            tool: "session_delete",
            arguments: json!({
                "selector": "id:019e32e3-0000-7000-8000-000000000000",
                "signal": "SIGTERM",
                "grace_secs": 5
            }),
        },
    ]
}
