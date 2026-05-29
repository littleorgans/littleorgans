#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt::{Debug, Display};

#[derive(Debug, thiserror::Error)]
pub enum PortError<F> {
    #[error(transparent)]
    Fault(F),
    #[error(transparent)]
    Opaque(OpaqueFault),
}

impl<F> PortError<F> {
    pub fn local(err: impl Display) -> Self {
        Self::Opaque(OpaqueFault::local(err))
    }

    pub fn wire(err: impl Error + Send + Sync + 'static) -> Self {
        Self::Opaque(OpaqueFault::wire(err))
    }
}

/// Opaque downstream failure.
///
/// `OpaqueKind` is private, so callers can match `PortError::Opaque(_)` but
/// cannot branch on local-versus-wire provenance.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct OpaqueFault(OpaqueKind);

#[derive(Debug, thiserror::Error)]
enum OpaqueKind {
    #[error("{0}")]
    Local(String),
    #[error("{0}")]
    Wire(#[source] Box<dyn Error + Send + Sync>),
}

impl OpaqueFault {
    fn local(err: impl Display) -> Self {
        Self(OpaqueKind::Local(err.to_string()))
    }

    fn wire(err: impl Error + Send + Sync + 'static) -> Self {
        Self(OpaqueKind::Wire(Box::new(err)))
    }
}

pub struct ParityProof(());

// Signature is the design contract. Callers choose whether E is owned or borrowed.
#[allow(clippy::needless_pass_by_value)]
pub fn prove_eq<E: PartialEq + Debug>(direct: E, via_socket: E) -> ParityProof {
    assert_eq!(direct, via_socket, "adapter parity violated");
    ParityProof(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[derive(Debug, thiserror::Error)]
    #[error("test fault")]
    struct TestFault;

    #[test]
    fn opaque_wire_preserves_source() {
        let error = PortError::<TestFault>::wire(std::io::Error::other("inner"));

        let source = error.source().expect("wire error keeps source");
        assert_eq!(source.to_string(), "inner");
    }

    #[test]
    fn opaque_local_has_no_source() {
        let error = PortError::<TestFault>::local("boom");

        assert!(error.source().is_none());
        assert_eq!(error.to_string(), "boom");
    }

    #[test]
    fn display_delegates() {
        assert_eq!(PortError::Fault(TestFault).to_string(), "test fault");
        assert_eq!(
            PortError::<TestFault>::local("opaque").to_string(),
            "opaque"
        );
    }

    #[test]
    fn prove_eq_returns_proof_on_equal() {
        let _proof = prove_eq(42, 42);
    }

    #[test]
    #[should_panic(expected = "adapter parity violated")]
    fn prove_eq_panics_on_mismatch() {
        let _proof = prove_eq(1, 2);
    }
}
