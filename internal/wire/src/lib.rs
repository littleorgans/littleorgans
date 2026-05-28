use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "substrate", content = "payload", rename_all = "lowercase")]
pub enum LilodRpc {
    Session(lilo_session_core::SessionRpc),
    Runtime(lilo_rm_core::RuntimeRpc),
}
