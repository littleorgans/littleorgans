use crate::Result;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerCred {
    pub uid: u32,
    pub gid: u32,
    pub pid: Option<u32>,
}

pub fn current_uid() -> u32 {
    crate::sys::current_uid()
}

pub fn peer_cred(stream: &crate::ipc::IpcStream) -> Result<PeerCred> {
    crate::sys::peer_cred(stream.as_raw_fd())
}
