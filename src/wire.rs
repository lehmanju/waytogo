use std::{
    ffi::{CString, IntoStringError},
    io,
    os::unix::prelude::RawFd,
    sync::{atomic::AtomicU32, Arc},
    task::Poll,
};

use bytes::BytesMut;
use futures::{Sink, Stream};
use pin_project_lite::pin_project;
use smallvec::SmallVec;
use thiserror::Error;
use tokio_util::{
    codec::{Decoder, Encoder},
    sync::PollSendError,
};

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
    #[error("unknown opcode `{0}`")]
    UnknownOpcode(u16),
    #[error("convert string failed `{0}`")]
    IntroStringError(#[from] IntoStringError),
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

pub trait WaylandInterface {
    type Event;
    type Request;
    /// Process a wayland message.
    ///
    /// This method expects a Message directly from the socket that corresponds to this wayland object. It returns an optional event that is sent to listeners further down the chain.
    /// Behavior is similar to StreamExt.scan
    fn process(&mut self, message: Message) -> Result<Option<Self::Event>, WaylandError>;
    /// Map a request from this interface to a wayland request.
    ///
    /// The request is then sent directly to the socket.
    fn request(&mut self, request: Self::Request) -> Result<Option<Message>, WaylandError>;
}

pin_project! {
    #[project=Proj]
    pub struct WlObject<T: Sink<Message>, R: Stream<Item = Message>, D: WaylandInterface> {
        pub id_counter: Arc<AtomicU32>,
        pub id: u32,
        #[pin]
        pub request_tx: T,
        #[pin]
        pub message_rx: R,
        pub data: D,
    }
}

impl<T, R, D> Stream for WlObject<T, R, D>
where
    T: Sink<Message>,
    R: Stream<Item = Message>,
    D: WaylandInterface,
{
    type Item = Result<D::Event, WaylandError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let Proj {
            id_counter: _,
            id,
            request_tx: _,
            mut message_rx,
            data,
        } = self.project();
        match message_rx.as_mut().poll_next(cx) {
            Poll::Ready(value) => match value {
                Some(message) => {
                    assert_eq!(*id, message.sender_id);
                    match data.process(message)? {
                        Some(event) => Poll::Ready(Some(Ok(event))),
                        None => Poll::Pending,
                    }
                }
                None => Poll::Pending,
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T, R, D> Sink<Message> for WlObject<T, R, D>
where
    T: Sink<Message, Error = WaylandError>,
    R: Stream<Item = Message>,
    D: WaylandInterface,
{
    type Error = WaylandError;

    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let Proj {
            id_counter: _,
            id: _,
            request_tx,
            message_rx: _,
            data: _,
        } = self.project();
        request_tx.poll_ready(cx)
    }

    fn start_send(self: std::pin::Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        let Proj {
            id_counter: _,
            id,
            request_tx,
            message_rx: _,
            data: _,
        } = self.project();
        assert_eq!(*id, item.sender_id);
        request_tx.start_send(item)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let Proj {
            id_counter: _,
            id: _,
            request_tx,
            message_rx: _,
            data: _,
        } = self.project();
        request_tx.poll_flush(cx)
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let Proj {
            id_counter: _,
            id: _,
            request_tx,
            message_rx: _,
            data: _,
        } = self.project();
        request_tx.poll_close(cx)
    }
}
