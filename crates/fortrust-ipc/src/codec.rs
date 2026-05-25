use bincode::{config, Decode, Encode};
use bytes::{Buf, BufMut, BytesMut};
use thiserror::Error;
use tracing::{debug, warn};

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error("Message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },
    #[error("Incomplete message: need {needed} more bytes")]
    Incomplete { needed: usize },
}

const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
const HEADER_SIZE: usize = 8;

pub struct FramedMessage {
    pub data: Vec<u8>,
}

pub struct BincodeCodec;

impl BincodeCodec {
    pub fn encode<T: Encode>(message: &T) -> Result<Vec<u8>, CodecError> {
        let config = config::standard()
            .with_big_endian()
            .with_variable_int_encoding();

        match bincode::encode_to_vec(message, config) {
            Ok(payload) => {
                if payload.len() > MAX_MESSAGE_SIZE {
                    return Err(CodecError::MessageTooLarge {
                        size: payload.len(),
                        max: MAX_MESSAGE_SIZE,
                    });
                }

                let mut buf = BytesMut::with_capacity(HEADER_SIZE + payload.len());
                buf.put_u64(payload.len() as u64);
                buf.put_slice(&payload);
                debug!("Encoded message: {} bytes payload", payload.len());
                Ok(buf.to_vec())
            }
            Err(error) => {
                warn!("Failed to encode message: {error}");
                Err(CodecError::Serialization(error.to_string()))
            }
        }
    }

    pub fn decode<T: Decode<()>>(data: &[u8]) -> Result<T, CodecError> {
        let config = config::standard()
            .with_big_endian()
            .with_variable_int_encoding();

        match bincode::decode_from_slice(data, config) {
            Ok((message, _)) => Ok(message),
            Err(error) => {
                warn!("Failed to decode message: {error}");
                Err(CodecError::Deserialization(error.to_string()))
            }
        }
    }

    pub fn decode_message<T: Decode<()>>(buffer: &mut BytesMut) -> Result<Option<(T, usize)>, CodecError> {
        if buffer.len() < HEADER_SIZE {
            return Ok(None);
        }

        let payload_len = u64::from_be_bytes([
            buffer[0], buffer[1], buffer[2], buffer[3],
            buffer[4], buffer[5], buffer[6], buffer[7],
        ]) as usize;

        if payload_len > MAX_MESSAGE_SIZE {
            return Err(CodecError::MessageTooLarge {
                size: payload_len,
                max: MAX_MESSAGE_SIZE,
            });
        }

        let total_len = HEADER_SIZE + payload_len;
        if buffer.len() < total_len {
            return Ok(None);
        }

        let payload = &buffer[HEADER_SIZE..total_len];
        let message = Self::decode(payload)?;

        buffer.advance(total_len);
        Ok(Some((message, total_len)))
    }
}

impl FramedMessage {
    pub fn new<T: Encode>(message: &T) -> Result<Self, CodecError> {
        let data = BincodeCodec::encode(message)?;
        Ok(Self { data })
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}
