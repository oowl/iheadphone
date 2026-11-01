use std::mem::discriminant;
use std::time::Instant;

use crate::Frame;
use crate::command::{Reply, Request, parse, parse_table_2};
use crate::frame::{COMMAND, COMMAND_2, FrameError, FrameReader, Message};
use crate::queue::StopAndWait;

#[derive(Default)]
pub(crate) struct Session {
    sequence: u8,
    reader: FrameReader,
    requests: StopAndWait<Request>,
}

#[derive(Debug, Default)]
pub(crate) struct Received {
    pub writes: Vec<Frame>,
    pub replies: Vec<Reply>,
    pub errors: Vec<FrameError>,
}

impl Session {
    pub fn request(&mut self, request: Request, now: Instant) -> Vec<Frame> {
        let coalesces = matches!(
            request,
            Request::SetNoiseControl(_) | Request::SetEqualizerBands(_)
        );
        let request_kind = discriminant(&request);
        self.requests.push(request, |queued| {
            coalesces && discriminant(queued) == request_kind
        });
        self.send_next(now).into_iter().collect()
    }

    pub fn receive(&mut self, bytes: &[u8], now: Instant) -> Received {
        let mut received = Received::default();
        for frame in self.reader.push(bytes) {
            let message = match frame {
                Ok(message) => message,
                Err(error) => {
                    received.errors.push(error);
                    continue;
                }
            };
            if message.is_ack() {
                if !message.payload.is_empty() || message.seq != (self.sequence ^ 1) {
                    received.errors.push(FrameError::UnexpectedAck);
                } else if self.requests.complete() {
                    self.sequence = message.seq;
                }
                continue;
            }

            received.writes.push(
                Message::ack_for(message.seq)
                    .encode()
                    .expect("validated sequence produces a valid ACK"),
            );
            let reply = match message.kind {
                COMMAND => parse(&message.payload),
                COMMAND_2 => parse_table_2(&message.payload),
                _ => None,
            };
            if let Some(reply) = reply {
                received.replies.push(reply);
            }
        }
        received.writes.extend(self.send_next(now));
        received
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Frame> {
        self.requests
            .tick(now)
            .map(|request| self.encode(request))
            .into_iter()
            .collect()
    }

    fn send_next(&mut self, now: Instant) -> Option<Frame> {
        self.requests.next(now).map(|request| self.encode(request))
    }

    fn encode(&self, request: Request) -> Frame {
        Message::new(request.kind(), self.sequence, request.payload())
            .encode()
            .expect("built-in XM5 commands fit the protocol frame")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn sends_one_request_at_a_time_and_advances_on_ack() {
        let now = Instant::now();
        let mut session = Session::default();
        assert_eq!(
            session.request(Request::Init, now),
            [Message::command(0, vec![0x00, 0x00]).encode().unwrap()]
        );
        assert!(session.request(Request::Battery, now).is_empty());
        let ack = Message {
            kind: crate::frame::ACK,
            seq: 1,
            payload: Vec::new(),
        }
        .encode()
        .unwrap();
        assert_eq!(
            session.receive(&ack, now).writes,
            [Message::command(1, vec![0x22, 0x00]).encode().unwrap()]
        );
    }

    #[test]
    fn keeps_errors_and_recovers_for_the_next_frame() {
        let now = Instant::now();
        let mut session = Session::default();
        let mut bad = Message::command(0, vec![0x22, 0x00]).encode().unwrap();
        bad[8] ^= 1;
        let valid = Message::command(0, vec![0x23, 0x00, 50, 0])
            .encode()
            .unwrap();
        bad.extend(valid);
        let received = session.receive(&bad, now);
        assert_eq!(received.errors, [FrameError::BadChecksum]);
        assert_eq!(received.replies.len(), 1);
    }

    #[test]
    fn retries_unanswered_commands() {
        let now = Instant::now();
        let mut session = Session::default();
        let first = session.request(Request::Battery, now);
        assert_eq!(session.tick(now + Duration::from_secs(1)), first);
    }

    #[test]
    fn rejects_stale_or_nonempty_acknowledgements() {
        let now = Instant::now();
        let mut session = Session::default();
        session.request(Request::Battery, now);
        for message in [
            Message {
                kind: crate::frame::ACK,
                seq: 0,
                payload: Vec::new(),
            },
            Message {
                kind: crate::frame::ACK,
                seq: 1,
                payload: vec![0xff],
            },
        ] {
            assert_eq!(
                session.receive(&message.encode().unwrap(), now).errors,
                [FrameError::UnexpectedAck]
            );
        }
    }
}
