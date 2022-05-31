use std::{
    collections::{HashMap, VecDeque},
    ffi::{CString, IntoStringError},
    io::{self, IoSliceMut},
    os::unix::{
        net::UnixStream,
        prelude::{AsRawFd, RawFd},
    },
    path::Path,
    sync::{atomic::AtomicU32, Arc},
    task::Poll,
};

use bytes::{Buf, Bytes, BytesMut};
use enum_as_inner::EnumAsInner;
use futures::{Sink, Stream};
use nix::{cmsg_space, errno::Errno, sys::socket};
use pin_project_lite::pin_project;
use smallvec::SmallVec;
use thiserror::Error;
use tokio::io::unix::AsyncFd;
use tokio_util::sync::PollSendError;

#[derive(Debug, Clone)]
pub struct Header {
    pub object_id: u32,
    pub message_size: u16,
    pub opcode: u16,
}

#[derive(Debug, Clone)]
pub struct Message {
    /// ID of the object sending this message
    pub sender_id: u32,
    /// Opcode of the message
    pub opcode: u16,
    /// Arguments of the message
    pub args: SmallVec<[Argument; INLINE_ARGS]>,
}

#[derive(Debug, Clone)]
pub struct RawMessage {
    pub header: Header,
    /// Arguments of the message
    pub args: Bytes,
}

const INLINE_ARGS: usize = 4;

#[derive(Debug, Clone, EnumAsInner)]
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

pub enum ArgumentType {
    /// i32
    Int,
    /// u32
    Uint,
    /// fixed point, 1/256 precision
    Fixed,
    /// CString
    ///
    /// The value is boxed to reduce the stack size of Argument. The performance
    /// impact is negligible as `string` arguments are pretty rare in the protocol.
    Str,
    /// id of a wayland object
    Object,
    /// id of a newly created wayland object
    NewId,
    /// Vec<u8>
    ///
    /// The value is boxed to reduce the stack size of Argument. The performance
    /// impact is negligible as `array` arguments are pretty rare in the protocol.
    Array,
    /// RawFd
    Fd,
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
    #[error("unix error `{0}`")]
    UnixError(#[from] Errno),
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

pub const MAX_FDS_OUT: usize = 28;
pub const MAX_BYTES_OUT: usize = 4096;

pub struct WlSocket {
    inner: AsyncFd<UnixStream>,
    ancillary_buffer: Vec<u8>,
    read_buffer: BytesMut,
    write_buffer: BytesMut,
    fds: VecDeque<RawFd>,
    header: Option<Header>,
    unprocessed_msg: Option<RawMessage>,
}

impl WlSocket {
    pub fn connect<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let socket = UnixStream::connect(path)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            inner: AsyncFd::new(socket)?,
            ancillary_buffer: cmsg_space!([RawFd; MAX_FDS_OUT]),
            read_buffer: BytesMut::new(),
            write_buffer: BytesMut::new(),
            fds: VecDeque::new(),
            header: None,
            unprocessed_msg: None,
        })
    }

    fn decode_raw(&mut self) -> Result<Option<RawMessage>, WaylandError> {
        const HEADER_LEN: usize = 8;

        if let Some(header) = self.header.take() {
            // header already received, try to parse message
            let remaining_bytes = header.message_size as usize - HEADER_LEN;
            if self.read_buffer.remaining() < remaining_bytes {
                self.header = Some(header);
                return Ok(None);
            } else {
                // complete message in buffer
                let args = self.read_buffer.copy_to_bytes(remaining_bytes);
                Ok(Some(RawMessage { header, args }))
            }
        } else {
            // no header received yet, try to parse header
            if self.read_buffer.remaining() < HEADER_LEN {
                self.read_buffer.reserve(HEADER_LEN);
                return Ok(None);
            }

            let object_id = self.read_buffer.get_u32();
            let message_size = self.read_buffer.get_u16();
            let opcode = self.read_buffer.get_u16();

            self.header = Some(Header {
                object_id,
                message_size,
                opcode,
            });
            let remaining_bytes = message_size as usize - HEADER_LEN;
            self.read_buffer.reserve(remaining_bytes + HEADER_LEN);

            self.decode_raw()
        }
    }

    fn decode(
        &mut self,
        map: &HashMap<u32, &phf::Map<u16, &[ArgumentType]>>,
    ) -> Result<Option<Message>, WaylandError> {
        if let Some(mut raw_message) = self.unprocessed_msg.take() {
            let event_map = map.get(&raw_message.header.object_id).unwrap();
            let argument_list = *event_map.get(&raw_message.header.opcode).unwrap();
            if argument_list.len() > INLINE_ARGS {
                panic!("Too many arguments for message")
            }
            let mut args = SmallVec::new();
            let mut used_fds = VecDeque::new();
            for arg in argument_list {
                let argument = match arg {
                    ArgumentType::Int => Argument::Int(raw_message.args.get_i32()),
                    ArgumentType::Uint => Argument::Uint(raw_message.args.get_u32()),
                    ArgumentType::Fixed => Argument::Fixed(raw_message.args.get_i32()),
                    ArgumentType::Str => {
                        let string_length = raw_message.args.get_u32() - 1;
                        let string_bytes = raw_message.args.copy_to_bytes(string_length as usize);
                        let string = CString::new(string_bytes.to_vec()).unwrap();
                        Argument::Str(Box::new(string))
                    }
                    ArgumentType::Object => Argument::Object(raw_message.args.get_u32()),
                    ArgumentType::NewId => Argument::NewId(raw_message.args.get_u32()),
                    ArgumentType::Array => todo!(),
                    ArgumentType::Fd => match self.fds.pop_front() {
                        Some(fd) => {
                            used_fds.push_front(fd);
                            Argument::Fd(fd)
                        }
                        None => {
                            for fd in used_fds {
                                self.fds.push_front(fd);
                            }
                            return Ok(None);
                        }
                    },
                };
                args.push(argument);
            }
            Ok(Some(Message {
                sender_id: raw_message.header.object_id,
                opcode: raw_message.header.opcode,
                args,
            }))
        } else {
            self.unprocessed_msg = self.decode_raw()?;
            if self.unprocessed_msg.is_none() {
                return Ok(None);
            }
            self.decode(map)
        }
    }

    fn encode(&mut self, msg: Message, buffer: &mut BytesMut) -> Result<Vec<RawFd>, WaylandError> {
        todo!()
    }

    pub async fn read_message(
        &mut self,
        map: &HashMap<u32, &phf::Map<u16, &[ArgumentType]>>,
    ) -> Result<Message, WaylandError> {
        loop {
            let mut guard = self.inner.readable().await?;
            match guard.try_io(|inner| {
                let mut iov = [IoSliceMut::new(&mut self.read_buffer)];
                let msg = socket::recvmsg::<()>(
                    inner.as_raw_fd(),
                    &mut iov[..],
                    Some(&mut self.ancillary_buffer),
                    socket::MsgFlags::MSG_DONTWAIT,
                )?;
                for cmsg in msg.cmsgs() {
                    match cmsg {
                        socket::ControlMessageOwned::ScmRights(fd) => self.fds.extend(fd.iter()),
                        _ => {} //ignore
                    }
                }
                Ok(msg.bytes)
            }) {
                Ok(result) => result?,
                Err(_) => continue,
            };
            return match self.decode(map)? {
                Some(message) => Ok(message),
                None => continue,
            };
        }
    }

    pub async fn write_message(&mut self, message: Message) -> Result<(), WaylandError> {
        let mut guard = self.inner.writable().await?;
        /*guard.try_io(|inner| {

        })?;*/
        Ok(())
    }
}
