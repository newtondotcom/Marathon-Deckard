//! The camera socket's messages (docs/camera.md). One SOCK_SEQPACKET
//! datagram per message: a little-endian `u32` type, then its fields. The
//! HAL's copy is `hardware/deckard/camera/HostCamera.h` in the image.

pub const VERSION: u32 = 1;

pub const HELLO: u32 = 1;
pub const OPEN: u32 = 2;
pub const START: u32 = 3;
pub const STOP: u32 = 4;
pub const RELEASE: u32 = 5;

pub const CAMERA: u32 = 101;
pub const CAMERAS_END: u32 = 102;
pub const OK: u32 = 103;
pub const ERROR: u32 = 104;
pub const FRAME: u32 = 105;
pub const FAILED: u32 = 106;

/// `ERROR` codes.
pub const BUSY: u32 = 1;
pub const GONE: u32 = 2;
pub const BROKEN: u32 = 3;
pub const INVALID: u32 = 4;

/// Sizes one `CAMERA` may list.
pub const MAX_SIZES: usize = 32;

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Hello { version: u32 },
    Open { camera: u32 },
    Start { width: u32, height: u32 },
    Stop,
    Release { frame: u64 },
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

impl Request {
    /// A request, if `bytes` is exactly one.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let request = match u32_at(bytes, 0)? {
            HELLO if bytes.len() == 8 => Self::Hello {
                version: u32_at(bytes, 4)?,
            },
            OPEN if bytes.len() == 8 => Self::Open {
                camera: u32_at(bytes, 4)?,
            },
            START if bytes.len() == 12 => Self::Start {
                width: u32_at(bytes, 4)?,
                height: u32_at(bytes, 8)?,
            },
            STOP if bytes.len() == 4 => Self::Stop,
            RELEASE if bytes.len() == 12 => Self::Release {
                frame: u64_at(bytes, 4)?,
            },
            _ => return None,
        };
        Some(request)
    }
}

/// Builds a message from its type and fields.
#[derive(Default)]
pub struct Message(Vec<u8>);

impl Message {
    #[must_use]
    pub fn new(kind: u32) -> Self {
        Self(kind.to_le_bytes().to_vec())
    }

    #[must_use]
    pub fn u32(mut self, value: u32) -> Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }

    #[must_use]
    pub fn u64(mut self, value: u64) -> Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_parse_only_at_their_exact_length() {
        let start = Message::new(START).u32(1280).u32(720);
        assert_eq!(
            Request::parse(start.bytes()),
            Some(Request::Start {
                width: 1280,
                height: 720
            })
        );
        assert_eq!(Request::parse(&start.bytes()[..8]), None);
        let release = Message::new(RELEASE).u64(7);
        assert_eq!(
            Request::parse(release.bytes()),
            Some(Request::Release { frame: 7 })
        );
        assert_eq!(Request::parse(Message::new(STOP).u32(0).bytes()), None);
        assert_eq!(Request::parse(Message::new(FRAME).bytes()), None);
    }
}
