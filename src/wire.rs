use std::{ffi::CString, io, os::unix::prelude::RawFd};

use bytes::BytesMut;
use smallvec::SmallVec;
use thiserror::Error;
use tokio_util::{codec::{Decoder, Encoder}, sync::PollSendError};

pub struct WaylandProtocol {}

#[derive(Debug, Clone)]
pub struct Message {
    /// ID of the object sending this message
    pub sender_id: u32,
    /// Opcode of the message
    pub opcode: u16,
    /// Arguments of the message
    pub args: SmallVec<[Argument; INLINE_ARGS]>,
}


const INLINE_ARGS: usize = 4;

#[derive(Debug, Clone)]
pub enum Argument {
    /// i32
    Int(i32),
    /// u32
    Uint(u32),
    /// fixed point, 1/256 precision
    Fixed(i32),
    /// CString
    ///
    /// The value is boxed to reduce the stack size of Argument. The performance
    /// impact is negligible as `string` arguments are pretty rare in the protocol.
    Str(Box<CString>),
    /// id of a wayland object
    Object(u32),
    /// id of a newly created wayland object
    NewId(u32),
    /// Vec<u8>
    ///
    /// The value is boxed to reduce the stack size of Argument. The performance
    /// impact is negligible as `array` arguments are pretty rare in the protocol.
    Array(Box<Vec<u8>>),
    /// RawFd
    Fd(RawFd),
}

#[derive(Error, Debug)]
pub enum WaylandError {
    #[error("parse error")]
    ParseError,
    #[error("io error `{0}`")]
    IoError(#[from] io::Error),
    #[error("poll error `{0}`")]
    PollError(#[from] PollSendError<Message>),
}

impl Decoder for WaylandProtocol {
    type Item = Message;

    type Error = WaylandError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        todo!()
    }
}

impl Encoder<Message> for WaylandProtocol {
    type Error = WaylandError;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        todo!()
    }
}
