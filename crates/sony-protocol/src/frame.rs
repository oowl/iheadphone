//! Sony MDR framing over RFCOMM.
//!
//! Adapted from `aecca/sound-connect` under MIT. This version adds bounded
//! buffering, payload limits, strict escape validation and sequence checks.

const START: u8 = 0x3e;
const END: u8 = 0x3c;
const ESCAPE: u8 = 0x3d;
const ESCAPE_MASK: u8 = 0xef;
const MAX_PAYLOAD: usize = 4 * 1024;
const MAX_BUFFER: usize = MAX_PAYLOAD * 2 + 32;

pub(crate) const ACK: u8 = 0x01;
pub(crate) const COMMAND: u8 = 0x0c;
pub(crate) const COMMAND_2: u8 = 0x0e;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub kind: u8,
    pub seq: u8,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    BufferOverflow,
    TooShort,
    PayloadTooLarge,
    InvalidEscape,
    InvalidSequence,
    UnexpectedAck,
    BadChecksum,
    BadLength,
}

impl Message {
    pub fn new(kind: u8, seq: u8, payload: Vec<u8>) -> Self {
        Self { kind, seq, payload }
    }

    #[cfg(test)]
    pub fn command(seq: u8, payload: Vec<u8>) -> Self {
        Self::new(COMMAND, seq, payload)
    }

    pub fn ack_for(seq: u8) -> Self {
        Self {
            kind: ACK,
            seq: seq ^ 1,
            payload: Vec::new(),
        }
    }

    pub fn is_ack(&self) -> bool {
        self.kind == ACK
    }

    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        if self.seq > 1 {
            return Err(FrameError::InvalidSequence);
        }
        if self.payload.len() > MAX_PAYLOAD || self.payload.len() > u32::MAX as usize {
            return Err(FrameError::PayloadTooLarge);
        }

        let mut body = Vec::with_capacity(self.payload.len() + 7);
        body.push(self.kind);
        body.push(self.seq);
        body.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        body.extend_from_slice(&self.payload);
        body.push(checksum(&body));

        let mut encoded = Vec::with_capacity(body.len() + 4);
        encoded.push(START);
        for byte in body {
            if matches!(byte, START | END | ESCAPE) {
                encoded.extend_from_slice(&[ESCAPE, byte & ESCAPE_MASK]);
            } else {
                encoded.push(byte);
            }
        }
        encoded.push(END);
        Ok(encoded)
    }

    fn decode_body(escaped: &[u8]) -> Result<Self, FrameError> {
        if escaped.len() > MAX_BUFFER {
            return Err(FrameError::PayloadTooLarge);
        }
        let mut bytes = Vec::with_capacity(escaped.len());
        let mut input = escaped.iter();
        while let Some(&byte) = input.next() {
            if byte != ESCAPE {
                bytes.push(byte);
                continue;
            }
            let escaped = *input.next().ok_or(FrameError::TooShort)?;
            let decoded = escaped | !ESCAPE_MASK;
            if !matches!(decoded, START | END | ESCAPE) {
                return Err(FrameError::InvalidEscape);
            }
            bytes.push(decoded);
        }

        if bytes.len() < 7 {
            return Err(FrameError::TooShort);
        }
        let expected_checksum = bytes.pop().expect("length checked");
        if checksum(&bytes) != expected_checksum {
            return Err(FrameError::BadChecksum);
        }
        if bytes[1] > 1 {
            return Err(FrameError::InvalidSequence);
        }
        let payload_len = u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
        if payload_len > MAX_PAYLOAD {
            return Err(FrameError::PayloadTooLarge);
        }
        if bytes.len() != 6 + payload_len {
            return Err(FrameError::BadLength);
        }
        Ok(Self {
            kind: bytes[0],
            seq: bytes[1],
            payload: bytes[6..].to_vec(),
        })
    }
}

fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte))
}

#[derive(Default)]
pub(crate) struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    pub fn push(&mut self, input: &[u8]) -> Vec<Result<Message, FrameError>> {
        let mut output = Vec::new();
        if self.buffer.len().saturating_add(input.len()) > MAX_BUFFER {
            self.buffer.clear();
            output.push(Err(FrameError::BufferOverflow));
        }
        if input.len() > MAX_BUFFER {
            return output;
        }
        self.buffer.extend_from_slice(input);

        while let Some(end) = self.buffer.iter().position(|&byte| byte == END) {
            let frame: Vec<u8> = self.buffer.drain(..=end).collect();
            if let Some(start) = frame.iter().rposition(|&byte| byte == START) {
                output.push(Message::decode_body(&frame[start + 1..frame.len() - 1]));
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_captured_xm5_frames() {
        assert_eq!(
            Message::command(0, vec![0x22, 0x00]).encode().unwrap(),
            [
                0x3e, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x02, 0x22, 0x00, 0x30, 0x3c
            ]
        );
    }

    #[test]
    fn reassembles_and_unescapes_split_frames() {
        let message = Message::command(1, vec![START, END, ESCAPE]);
        let encoded = message.encode().unwrap();
        let mut reader = FrameReader::default();
        assert!(reader.push(&encoded[..4]).is_empty());
        assert_eq!(reader.push(&encoded[4..]), [Ok(message)]);
    }

    #[test]
    fn rejects_invalid_escape_and_sequence() {
        let mut invalid_escape = Message::command(0, vec![START]).encode().unwrap();
        let position = invalid_escape
            .iter()
            .position(|&byte| byte == ESCAPE)
            .unwrap();
        invalid_escape[position + 1] = 0x00;
        assert_eq!(
            FrameReader::default().push(&invalid_escape),
            [Err(FrameError::InvalidEscape)]
        );
        assert_eq!(
            Message::command(2, Vec::new()).encode(),
            Err(FrameError::InvalidSequence)
        );
    }

    #[test]
    fn bounds_unterminated_input() {
        let mut reader = FrameReader::default();
        assert_eq!(
            reader.push(&vec![0xaa; MAX_BUFFER + 1]),
            [Err(FrameError::BufferOverflow)]
        );
        let valid = Message::command(0, vec![0x22, 0x00]);
        assert_eq!(reader.push(&valid.encode().unwrap()), [Ok(valid)]);
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut encoded = Message::command(0, vec![0x22, 0x00]).encode().unwrap();
        encoded[8] ^= 1;
        assert_eq!(
            FrameReader::default().push(&encoded),
            [Err(FrameError::BadChecksum)]
        );
    }
}
