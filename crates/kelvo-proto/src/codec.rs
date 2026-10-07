//! Framing and the CBOR codec (architecture.md infra 3).
//!
//! A frame is a `u32` big-endian body length followed by a CBOR body holding one
//! [`Message`]. Bodies larger than [`MAX_FRAME_LEN`] are rejected before allocating, so a
//! corrupt length cannot make the reader allocate gigabytes.

use std::io::{self, Read, Write};

use crate::message::Message;

/// Largest accepted frame body. A sync page of 30 days of one-minute buckets for 250
/// series is far smaller; anything bigger is a corrupt stream.
pub const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
    #[error("frame of {0} bytes exceeds the {MAX_FRAME_LEN} byte limit")]
    TooLarge(u64),
    #[error("cbor encode: {0}")]
    Encode(String),
    #[error("cbor decode: {0}")]
    Decode(String),
}

/// Encodes a message as a CBOR body (no length prefix).
pub fn encode_message(msg: &Message) -> Result<Vec<u8>, CodecError> {
    let mut body = Vec::new();
    ciborium::into_writer(msg, &mut body).map_err(|e| CodecError::Encode(e.to_string()))?;
    Ok(body)
}

/// Decodes one CBOR body into a message. A message type this build does not know
/// decodes as [`Message::Unknown`], whatever its content.
///
/// `#[serde(other)]` alone is not enough for that: on an adjacently tagged enum it only
/// accepts an unknown tag whose content is absent or null, and fails on a map, array or
/// scalar (D-038). So when the normal decode fails, this peeks at the tag; an unknown tag
/// becomes `Unknown`, and a known tag with a broken body stays an error. The happy path
/// decodes once.
pub fn decode_message(body: &[u8]) -> Result<Message, CodecError> {
    match ciborium::from_reader::<Message, _>(body) {
        Ok(msg) => Ok(msg),
        Err(err) => match peek_tag(body) {
            Some(tag) if !KNOWN_MESSAGE_TAGS.contains(&tag.as_str()) => Ok(Message::Unknown),
            _ => Err(CodecError::Decode(err.to_string())),
        },
    }
}

/// Every `t` value [`Message`] defines, except `Unknown`. A test in `tests/skew.rs` keeps
/// this list equal to the enum's variants.
pub const KNOWN_MESSAGE_TAGS: &[&str] = &[
    "Hello",
    "HelloAck",
    "Ping",
    "Pong",
    "Subscribe",
    "Unsubscribe",
    "LayoutDef",
    "Live",
    "LiveProcesses",
    "CapabilitiesChanged",
    "SyncRequest",
    "SyncPage",
    "Truncated",
    "HostSummary",
    "Error",
];

/// Reads only the `t` field of a message body, skipping the content.
fn peek_tag(body: &[u8]) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Tag {
        t: String,
    }
    ciborium::from_reader::<Tag, _>(body).ok().map(|t| t.t)
}

/// Encodes a message as a complete frame: length prefix plus body.
pub fn encode_frame(msg: &Message) -> Result<Vec<u8>, CodecError> {
    let body = encode_message(msg)?;
    let len = u32::try_from(body.len())
        .ok()
        .filter(|&n| n <= MAX_FRAME_LEN)
        .ok_or(CodecError::TooLarge(body.len() as u64))?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Writes one frame.
pub fn write_frame<W: Write>(w: &mut W, msg: &Message) -> Result<(), CodecError> {
    w.write_all(&encode_frame(msg)?)?;
    Ok(())
}

/// Reads one frame. `Ok(None)` on a clean end of stream before a new frame starts; an end
/// of stream inside a frame is an error.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Option<Message>, CodecError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < len.len() {
        let rest = len.get_mut(got..).unwrap_or_default();
        match r.read(rest) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    let len = u32::from_be_bytes(len);
    if len > MAX_FRAME_LEN {
        return Err(CodecError::TooLarge(u64::from(len)));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;
    decode_message(&body).map(Some)
}
