//! Packed column formats: tier blobs (f32 LE triples), layout series lists (u32 LE) and
//! process rows, and per-app network rows. All little-endian and fixed width; only the
//! network rows have a header.

use crate::error::{Result, StoreError};

pub(crate) fn pack_f32s(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

pub(crate) fn unpack_f32s(blob: &[u8]) -> Result<Vec<f32>> {
    if !blob.len().is_multiple_of(4) {
        return Err(StoreError::Corrupt(format!(
            "f32 blob of {} bytes",
            blob.len()
        )));
    }
    Ok(blob
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect())
}

pub(crate) fn pack_u32s(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

pub(crate) fn unpack_u32s(blob: &[u8]) -> Result<Vec<u32>> {
    if !blob.len().is_multiple_of(4) {
        return Err(StoreError::Corrupt(format!(
            "u32 blob of {} bytes",
            blob.len()
        )));
    }
    Ok(blob
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect())
}

/// FNV-1a 64 of a layout's packed series IDs: the `layouts.hash` dedupe key. A match is
/// confirmed by comparing `series_ids`, so a collision is an error, never a merge.
pub(crate) fn layout_hash(series_ids: &[u8]) -> [u8; 8] {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in series_ids {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h.to_le_bytes()
}

/// One packed process: (name_id u32, pid i32, cpu f32, mem KiB u32, threads u32,
/// wakeups f32, energy f32) = 28 bytes, as the budget in architecture.md assumes.
pub(crate) const PROC_ROW_BYTES: usize = 28;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PackedProc {
    pub name_id: u32,
    pub pid: i32,
    pub cpu: f32,
    pub mem_kib: u32,
    pub threads: u32,
    pub wakeups: f32,
    pub energy: f32,
}

pub(crate) fn pack_procs(rows: &[PackedProc]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rows.len() * PROC_ROW_BYTES);
    for r in rows {
        out.extend_from_slice(&r.name_id.to_le_bytes());
        out.extend_from_slice(&r.pid.to_le_bytes());
        out.extend_from_slice(&r.cpu.to_le_bytes());
        out.extend_from_slice(&r.mem_kib.to_le_bytes());
        out.extend_from_slice(&r.threads.to_le_bytes());
        out.extend_from_slice(&r.wakeups.to_le_bytes());
        out.extend_from_slice(&r.energy.to_le_bytes());
    }
    out
}

pub(crate) fn unpack_procs(blob: &[u8]) -> Result<Vec<PackedProc>> {
    if !blob.len().is_multiple_of(PROC_ROW_BYTES) {
        return Err(StoreError::Corrupt(format!(
            "process blob of {} bytes",
            blob.len()
        )));
    }
    Ok(blob
        .as_chunks::<PROC_ROW_BYTES>()
        .0
        .iter()
        .map(|row| {
            // Seven 4-byte words; a 28-byte chunk always has all of them.
            let mut w = row.as_chunks::<4>().0.iter().copied();
            let mut next = || w.next().unwrap_or_default();
            PackedProc {
                name_id: u32::from_le_bytes(next()),
                pid: i32::from_le_bytes(next()),
                cpu: f32::from_le_bytes(next()),
                mem_kib: u32::from_le_bytes(next()),
                threads: u32::from_le_bytes(next()),
                wakeups: f32::from_le_bytes(next()),
                energy: f32::from_le_bytes(next()),
            }
        })
        .collect())
}

/// `name_id` of "other apps" in a network row: the apps below a bucket's top 20, and bytes
/// whose app has no name. Never an interned name: `proc_names` ids are rowids, which
/// SQLite starts at 1 and never assigns 0 to.
pub(crate) const OTHER_APPS: u32 = 0;

/// Header of a network blob: measured_ms u32, then interface rx bytes, tx bytes, rx
/// packets, tx packets as u64. 36 bytes.
pub(crate) const NET_HEADER_BYTES: usize = 36;
/// One app in a network blob: (name_id u32, rx u64, tx u64) = 20 bytes.
pub(crate) const NET_ROW_BYTES: usize = 20;

/// The interface side of a network bucket.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NetHeader {
    pub measured_ms: u32,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_pkts: u64,
    pub tx_pkts: u64,
}

impl NetHeader {
    /// Adds `other`'s span and counters. Saturating: a sum past `u64::MAX` is corrupt
    /// input, and capping it beats wrapping to a small number.
    pub fn add(&mut self, other: &NetHeader) {
        self.measured_ms = self.measured_ms.saturating_add(other.measured_ms);
        self.rx_bytes = self.rx_bytes.saturating_add(other.rx_bytes);
        self.tx_bytes = self.tx_bytes.saturating_add(other.tx_bytes);
        self.rx_pkts = self.rx_pkts.saturating_add(other.rx_pkts);
        self.tx_pkts = self.tx_pkts.saturating_add(other.tx_pkts);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedNetApp {
    pub name_id: u32,
    pub rx: u64,
    pub tx: u64,
}

pub(crate) fn pack_net(header: &NetHeader, rows: &[PackedNetApp]) -> Vec<u8> {
    let mut out = Vec::with_capacity(NET_HEADER_BYTES + rows.len() * NET_ROW_BYTES);
    out.extend_from_slice(&header.measured_ms.to_le_bytes());
    for v in [
        header.rx_bytes,
        header.tx_bytes,
        header.rx_pkts,
        header.tx_pkts,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for r in rows {
        out.extend_from_slice(&r.name_id.to_le_bytes());
        out.extend_from_slice(&r.rx.to_le_bytes());
        out.extend_from_slice(&r.tx.to_le_bytes());
    }
    out
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

pub(crate) fn unpack_net(blob: &[u8]) -> Result<(NetHeader, Vec<PackedNetApp>)> {
    let corrupt = || StoreError::Corrupt(format!("network blob of {} bytes", blob.len()));
    if blob.len() < NET_HEADER_BYTES
        || !(blob.len() - NET_HEADER_BYTES).is_multiple_of(NET_ROW_BYTES)
    {
        return Err(corrupt());
    }
    let header = NetHeader {
        measured_ms: u32_at(blob, 0).ok_or_else(corrupt)?,
        rx_bytes: u64_at(blob, 4).ok_or_else(corrupt)?,
        tx_bytes: u64_at(blob, 12).ok_or_else(corrupt)?,
        rx_pkts: u64_at(blob, 20).ok_or_else(corrupt)?,
        tx_pkts: u64_at(blob, 28).ok_or_else(corrupt)?,
    };
    let rows = blob
        .get(NET_HEADER_BYTES..)
        .unwrap_or_default()
        .as_chunks::<NET_ROW_BYTES>()
        .0
        .iter()
        .map(|row| {
            Some(PackedNetApp {
                name_id: u32_at(row, 0)?,
                rx: u64_at(row, 4)?,
                tx: u64_at(row, 12)?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(corrupt)?;
    Ok((header, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_blobs_round_trip() {
        let header = NetHeader {
            measured_ms: 9_500,
            rx_bytes: u64::MAX,
            tx_bytes: 1 << 40,
            rx_pkts: 7,
            tx_pkts: 0,
        };
        let rows = [
            PackedNetApp {
                name_id: 3,
                rx: 5_000_000,
                tx: 12,
            },
            PackedNetApp {
                name_id: OTHER_APPS,
                rx: 1,
                tx: u64::MAX,
            },
        ];
        let blob = pack_net(&header, &rows);
        assert_eq!(blob.len(), NET_HEADER_BYTES + 2 * NET_ROW_BYTES);
        assert_eq!(unpack_net(&blob).unwrap(), (header, rows.to_vec()));
        let empty = pack_net(&NetHeader::default(), &[]);
        assert_eq!(empty.len(), NET_HEADER_BYTES);
        assert_eq!(unpack_net(&empty).unwrap(), (NetHeader::default(), vec![]));
    }

    #[test]
    fn network_blobs_of_a_wrong_length_are_corrupt() {
        let blob = pack_net(
            &NetHeader::default(),
            &[PackedNetApp {
                name_id: 1,
                rx: 1,
                tx: 1,
            }],
        );
        for len in [0, 35, NET_HEADER_BYTES + 1, blob.len() - 1, blob.len() + 19] {
            let mut b = blob.clone();
            b.resize(len, 0);
            assert!(
                matches!(unpack_net(&b), Err(StoreError::Corrupt(_))),
                "{len} bytes"
            );
        }
    }

    #[test]
    fn round_trips() {
        let f = [1.5, f32::NAN, -0.0, f32::MAX];
        let back = unpack_f32s(&pack_f32s(&f)).unwrap();
        assert_eq!(back.len(), 4);
        assert!(back[1].is_nan());
        assert_eq!(back[3], f32::MAX);
        assert_eq!(
            unpack_u32s(&pack_u32s(&[7, u32::MAX])).unwrap(),
            [7, u32::MAX]
        );
        let p = PackedProc {
            name_id: 3,
            pid: -1,
            cpu: 120.5,
            mem_kib: 1 << 20,
            threads: 9,
            wakeups: 2.5,
            energy: 0.25,
        };
        let blob = pack_procs(&[p, p]);
        assert_eq!(blob.len(), 2 * PROC_ROW_BYTES);
        assert_eq!(unpack_procs(&blob).unwrap(), vec![p, p]);
    }

    #[test]
    fn rejects_ragged_blobs() {
        assert!(unpack_f32s(&[0; 5]).is_err());
        assert!(unpack_u32s(&[0; 3]).is_err());
        assert!(unpack_procs(&[0; 27]).is_err());
    }

    #[test]
    fn hash_depends_on_order() {
        assert_ne!(
            layout_hash(&pack_u32s(&[1, 2])),
            layout_hash(&pack_u32s(&[2, 1]))
        );
    }
}
