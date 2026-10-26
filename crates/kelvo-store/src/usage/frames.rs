//! One standard lossless column-major stream per normal five-minute commit.
//! Frame timestamps and row counts retain every finest ten-second interval.
use super::{UsageFacts, codec};
use crate::{Result, StoreError};
use std::collections::HashSet;

pub(crate) const BATCH_MS: i64 = 300_000;
pub(crate) type Frame = (i64, Vec<(u64, UsageFacts)>);
const CHUNK_ROWS: usize = 1_000_000;

pub(crate) fn encode(frames: &[Frame]) -> Result<Vec<u8>> {
    if frames.is_empty() || frames.len() > 30 {
        return Err(corrupt("too many frames"));
    }
    let mut out = vec![2, frames.len() as u8];
    let mut rows = Vec::new();
    for (at, facts) in frames {
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(
            &u32::try_from(facts.len())
                .map_err(|_| corrupt("oversized frame"))?
                .to_le_bytes(),
        );
        rows.extend(facts.iter().cloned());
    }
    let chunks = rows.len().div_ceil(CHUNK_ROWS);
    out.push(u8::try_from(chunks).map_err(|_| corrupt("too many chunks"))?);
    for chunk in rows.chunks(CHUNK_ROWS) {
        let blob = codec::encode(chunk)?;
        out.extend_from_slice(
            &u32::try_from(blob.len())
                .map_err(|_| corrupt("oversized chunk"))?
                .to_le_bytes(),
        );
        out.extend(blob);
    }
    Ok(out)
}

pub(crate) fn decode(blob: &[u8], at: i64, include: Option<&HashSet<u64>>) -> Result<Vec<Frame>> {
    if !kelvo_schema::is_js_safe(at) {
        return Err(corrupt("invalid batch time"));
    }
    if blob.first() != Some(&2) {
        return Ok(vec![(at, codec::decode_selected(blob, include)?)]);
    }
    let mut input = blob
        .get(1..)
        .ok_or_else(|| corrupt("missing frame header"))?;
    let count = usize::from(take::<1>(&mut input)?[0]);
    if count == 0 || count > 30 {
        return Err(corrupt("oversized frame count"));
    }
    let mut ends = Vec::with_capacity(count);
    let mut result = Vec::with_capacity(count);
    let mut total = 0;
    for _ in 0..count {
        let time = i64::from_le_bytes(take::<8>(&mut input)?);
        let n = u32::from_le_bytes(take::<4>(&mut input)?) as usize;
        if n > CHUNK_ROWS {
            return Err(corrupt("oversized frame"));
        }
        if !kelvo_schema::is_js_safe(time)
            || time < at
            || time >= at + BATCH_MS
            || time.rem_euclid(10_000) != 0
            || result.last().is_some_and(|(previous, _)| *previous >= time)
        {
            return Err(corrupt("invalid frame time"));
        }
        total += n;
        ends.push(total);
        result.push((time, Vec::new()));
    }
    let chunks = usize::from(take::<1>(&mut input)?[0]);
    if chunks != total.div_ceil(CHUNK_ROWS) {
        return Err(corrupt("chunk count mismatch"));
    }
    let mut offset = 0;
    for _ in 0..chunks {
        let n = u32::from_le_bytes(take::<4>(&mut input)?) as usize;
        let part = input.get(..n).ok_or_else(|| corrupt("truncated chunk"))?;
        let rows = u32::from_le_bytes(
            part.get(1..5)
                .and_then(|s| s.try_into().ok())
                .ok_or_else(|| corrupt("short chunk"))?,
        ) as usize;
        if rows != (total - offset).min(CHUNK_ROWS) {
            return Err(corrupt("row count mismatch"));
        }
        for (index, id, facts) in codec::decode_indexed(part, include)? {
            let frame = ends.partition_point(|end| *end <= offset + index);
            result
                .get_mut(frame)
                .ok_or_else(|| corrupt("invalid frame index"))?
                .1
                .push((id, facts));
        }
        offset += rows;
        input = &input[n..];
    }
    if !input.is_empty() {
        return Err(corrupt("trailing frame bytes"));
    }
    Ok(result)
}

fn take<const N: usize>(input: &mut &[u8]) -> Result<[u8; N]> {
    let bytes = input
        .get(..N)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| corrupt("truncated frame header"))?;
    *input = &input[N..];
    Ok(bytes)
}
fn corrupt(message: &str) -> StoreError {
    StoreError::Corrupt(format!("usage batch: {message}"))
}
