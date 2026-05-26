use anyhow::{Result, bail};
use lilo_rm_core::{CliOutput, Lifecycle, RuntimeResponse};

pub fn print_spawned(response: RuntimeResponse) -> Result<Lifecycle> {
    let RuntimeResponse::Spawned(payload) = response else {
        bail!("unexpected spawn response: {response:?}");
    };
    let mut rendered = String::new();
    let lifecycle = payload.lifecycle.clone();
    RuntimeResponse::Spawned(payload).render_human(&mut rendered)?;
    print!("{rendered}");

    Ok(lifecycle)
}
