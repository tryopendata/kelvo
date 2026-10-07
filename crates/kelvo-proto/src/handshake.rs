//! Version and feature negotiation for the handshake.
//!
//! Each side speaks every protocol version in `min_compatible..=proto_version`. The
//! connection uses the highest version in the intersection of the two ranges, and the
//! features both sides list.

use std::collections::BTreeSet;

use kelvo_schema::SyncKinds;

use crate::message::{Hello, HelloAck};

/// One side's offer: its version range and optional features.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub proto_version: u16,
    pub min_compatible: u16,
    pub features: BTreeSet<String>,
}

impl Offer {
    /// This build's offer with the given features. A v1 build lists every
    /// [`SyncKinds::ALL`] feature it syncs; see [`local_features`].
    pub fn local(features: BTreeSet<String>) -> Self {
        Self {
            proto_version: crate::PROTO_VERSION,
            min_compatible: crate::MIN_COMPATIBLE,
            features,
        }
    }
}

impl From<&Hello> for Offer {
    fn from(h: &Hello) -> Self {
        Self {
            proto_version: h.proto_version,
            min_compatible: h.min_compatible,
            features: h.features.clone(),
        }
    }
}

/// The outcome both sides agree on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Negotiated {
    pub proto_version: u16,
    pub features: BTreeSet<String>,
}

impl Negotiated {
    /// The `SyncPage` row kinds both sides agreed on. The sender reads only these, and
    /// the receiver stores them with its cursor.
    pub fn sync_kinds(&self) -> SyncKinds {
        SyncKinds::from_features(self.features.iter().map(String::as_str))
    }

    /// The controller's reply to the agent's `Hello`.
    pub fn ack(&self) -> HelloAck {
        HelloAck {
            proto_version: self.proto_version,
            features: self.features.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HandshakeError {
    #[error(
        "no common protocol version: we speak {local_min}..={local_max}, peer speaks {peer_min}..={peer_max}"
    )]
    Incompatible {
        local_min: u16,
        local_max: u16,
        peer_min: u16,
        peer_max: u16,
    },
    #[error("malformed version range {min}..={max}")]
    BadRange { min: u16, max: u16 },
}

/// The features this build offers: every `SyncPage` row kind it knows.
pub fn local_features() -> BTreeSet<String> {
    SyncKinds::ALL.features().map(str::to_string).collect()
}

/// Picks the protocol version and feature set for a connection, or explains why the two
/// sides cannot talk.
pub fn negotiate(local: &Offer, peer: &Offer) -> Result<Negotiated, HandshakeError> {
    for o in [local, peer] {
        if o.min_compatible > o.proto_version {
            return Err(HandshakeError::BadRange {
                min: o.min_compatible,
                max: o.proto_version,
            });
        }
    }
    let version = local.proto_version.min(peer.proto_version);
    if version < local.min_compatible.max(peer.min_compatible) {
        return Err(HandshakeError::Incompatible {
            local_min: local.min_compatible,
            local_max: local.proto_version,
            peer_min: peer.min_compatible,
            peer_max: peer.proto_version,
        });
    }
    Ok(Negotiated {
        proto_version: version,
        features: local
            .features
            .intersection(&peer.features)
            .cloned()
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(min: u16, max: u16, features: &[&str]) -> Offer {
        Offer {
            proto_version: max,
            min_compatible: min,
            features: features.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn equal_versions() {
        let n = negotiate(&offer(1, 1, &["a", "b"]), &offer(1, 1, &["b", "c"])).unwrap();
        assert_eq!(n.proto_version, 1);
        assert_eq!(n.features, ["b".to_string()].into());
        assert_eq!(n.ack().proto_version, 1);
    }

    #[test]
    fn sync_kinds_are_the_negotiated_row_features() {
        let new = Offer::local(local_features());
        assert_eq!(negotiate(&new, &new).unwrap().sync_kinds(), SyncKinds::ALL);
        // An older peer that knows only buckets and gaps never gets events.
        let old = offer(1, 1, &["rows.buckets", "rows.gaps", "zstd-pages"]);
        let kinds = negotiate(&new, &old).unwrap().sync_kinds();
        assert!(kinds.contains(kelvo_schema::SyncRowKind::Gaps));
        assert!(!kinds.contains(kelvo_schema::SyncRowKind::Events));
        // A peer that lists no row kinds syncs nothing.
        let bare = negotiate(&new, &offer(1, 1, &[])).unwrap();
        assert_eq!(bare.sync_kinds(), SyncKinds::NONE);
    }

    #[test]
    fn older_compatible_peer_uses_its_version() {
        // We speak 1..=3, an older agent speaks only 2.
        let n = negotiate(&offer(1, 3, &[]), &offer(2, 2, &[])).unwrap();
        assert_eq!(n.proto_version, 2);
        // And the reverse: we are the older side.
        let n = negotiate(&offer(2, 2, &[]), &offer(1, 3, &[])).unwrap();
        assert_eq!(n.proto_version, 2);
    }

    #[test]
    fn incompatible_peer_is_an_error() {
        // We need at least 3; the peer tops out at 2.
        let err = negotiate(&offer(3, 4, &[]), &offer(1, 2, &[])).unwrap_err();
        assert!(matches!(
            err,
            HandshakeError::Incompatible {
                local_min: 3,
                peer_max: 2,
                ..
            }
        ));
        // The reverse direction fails too.
        assert!(negotiate(&offer(1, 2, &[]), &offer(3, 4, &[])).is_err());
    }

    #[test]
    fn malformed_range_rejected() {
        assert_eq!(
            negotiate(&offer(1, 1, &[]), &offer(3, 2, &[])),
            Err(HandshakeError::BadRange { min: 3, max: 2 })
        );
    }

    #[test]
    fn local_offer_uses_crate_constants() {
        let o = Offer::local(BTreeSet::new());
        assert_eq!(
            (o.min_compatible, o.proto_version),
            (crate::MIN_COMPATIBLE, crate::PROTO_VERSION)
        );
        assert!(negotiate(&o, &o).is_ok());
    }
}
