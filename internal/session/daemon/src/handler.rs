mod authz;
mod dispatch;
mod mail_observability;
mod message_view;
mod messaging;
mod sessions;
mod spawn;
mod state;
mod target;

pub use state::{DaemonState, HandlerResult, MailAppendEvent};
