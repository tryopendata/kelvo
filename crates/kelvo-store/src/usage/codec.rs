//! Versioned, lossless column-major LE facts, compressed with standard DEFLATE.
use std::collections::HashSet;
use std::io::{Read, Write};

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

use super::{UsageFacts, UsageSourceCoverage};
use crate::{Result, StoreError};

const VERSION: u8 = 1;
const ROW_BYTES: usize = 133;
const MAX_ROWS: usize = 1_000_000;

pub(crate) fn encode(rows: &[(u64, UsageFacts)]) -> Result<Vec<u8>> {
    if rows.len() > MAX_ROWS {
        return Err(StoreError::Usage(
            "too many facts in one usage bucket".into(),
        ));
    }
    let mut raw = Vec::with_capacity(rows.len() * ROW_BYTES);
    for (id, f) in rows {
        raw.extend_from_slice(&id.to_le_bytes());
        for v in [f.cpu_seconds, f.gpu_seconds, f.energy_j] {
            if !v.is_finite() || v < 0.0 {
                return Err(StoreError::Usage("invalid usage integral".into()));
            }
            raw.extend_from_slice(&v.to_le_bytes());
        }
        raw.extend_from_slice(&f.mem_peak_bytes.to_le_bytes());
        if !f.mem_byte_ms.is_finite() || f.mem_byte_ms < 0.0 {
            return Err(StoreError::Usage("invalid memory integral".into()));
        }
        raw.extend_from_slice(&f.mem_byte_ms.to_le_bytes());
        for v in [f.present_ms, f.network_rx_bytes, f.network_tx_bytes] {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        for c in f.coverage {
            raw.extend_from_slice(&c.measured_ms.to_le_bytes());
        }
        for c in f.coverage {
            raw.extend_from_slice(&c.causes.to_le_bytes());
        }
        raw.push(
            f.coverage
                .iter()
                .enumerate()
                .fold(0, |m, (i, c)| m | (u8::from(c.supported) << i)),
        );
    }
    let mut shuffled = Vec::with_capacity(raw.len());
    for column in 0..ROW_BYTES {
        shuffled.extend(raw.iter().skip(column).step_by(ROW_BYTES));
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&shuffled)?;
    let mut out = vec![VERSION];
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    out.extend(encoder.finish()?);
    Ok(out)
}

pub(crate) fn decode(blob: &[u8]) -> Result<Vec<(u64, UsageFacts)>> {
    decode_selected(blob, None)
}

/// Filtering identities before decoding full facts keeps lazy expansion allocations
/// proportional to the app, although the compressed bucket is read once.
pub(crate) fn decode_selected(
    blob: &[u8],
    include: Option<&HashSet<u64>>,
) -> Result<Vec<(u64, UsageFacts)>> {
    let header = blob.get(..5).ok_or_else(|| corrupt("missing header"))?;
    if header.first().copied() != Some(VERSION) {
        return Err(corrupt("unsupported version"));
    }
    let count = u32::from_le_bytes(
        header
            .get(1..5)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| corrupt("missing count"))?,
    ) as usize;
    if count > MAX_ROWS {
        return Err(corrupt("oversized count"));
    }
    let expected = count * ROW_BYTES;
    let mut raw = Vec::new();
    ZlibDecoder::new(blob.get(5..).unwrap_or_default())
        .take(expected as u64 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() != expected {
        return Err(corrupt("facts length mismatch"));
    }
    let mut result = Vec::new();
    for row in 0..count {
        let byte = |column: usize| {
            raw.get(column * count + row)
                .copied()
                .ok_or_else(|| corrupt("short column"))
        };
        let word = |column: usize| -> Result<[u8; 8]> {
            let mut value = [0; 8];
            for (i, b) in value.iter_mut().enumerate() {
                *b = byte(column + i)?;
            }
            Ok(value)
        };
        let id = u64::from_le_bytes(word(0)?);
        if include.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let supported = byte(132)?;
        let mut coverage = [UsageSourceCoverage::default(); 5];
        for (i, c) in coverage.iter_mut().enumerate() {
            let mut causes = [0; 4];
            for (j, b) in causes.iter_mut().enumerate() {
                *b = byte(112 + i * 4 + j)?;
            }
            *c = UsageSourceCoverage {
                supported: supported & (1 << i) != 0,
                measured_ms: u64::from_le_bytes(word(72 + i * 8)?),
                causes: u32::from_le_bytes(causes),
            };
        }
        let facts = UsageFacts {
            cpu_seconds: f64::from_le_bytes(word(8)?),
            gpu_seconds: f64::from_le_bytes(word(16)?),
            energy_j: f64::from_le_bytes(word(24)?),
            mem_peak_bytes: u64::from_le_bytes(word(32)?),
            mem_byte_ms: f64::from_le_bytes(word(40)?),
            present_ms: u64::from_le_bytes(word(48)?),
            network_rx_bytes: u64::from_le_bytes(word(56)?),
            network_tx_bytes: u64::from_le_bytes(word(64)?),
            coverage,
        };
        if [
            facts.cpu_seconds,
            facts.gpu_seconds,
            facts.energy_j,
            facts.mem_byte_ms,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(corrupt("nonfinite or negative integral"));
        }
        result.push((id, facts));
    }
    Ok(result)
}

fn corrupt(message: &str) -> StoreError {
    StoreError::Corrupt(format!("usage facts: {message}"))
}
