use std::{
    collections::{HashMap, VecDeque},
    ffi::{CString, IntoStringError},
    io::{self, IoSlice, IoSliceMut},
    os::unix::{
        net::UnixStream,
        prelude::{AsRawFd, RawFd},
    },
    sync::{Arc, PoisonError, RwLock, RwLockWriteGuard},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use enum_as_inner::EnumAsInner;
use futures::{executor::block_on, Sink, SinkExt, Stream};
use nix::{cmsg_space, errno::Errno, sys::socket};
use smallvec::SmallVec;
use thiserror::Error;
use tokio::{
    io::unix::{AsyncFd, TryIoError},
    sync::mpsc::channel,
};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tokio_util::sync::{PollSendError, PollSender};

use crate::{connection::WlConnectionMessage, BufExt, BufMutExt};

pub type Signature = &'static [&'static [ArgumentType]];

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

impl Message {
    pub fn is_valid(&self, signature: Signature, id: u32) -> bool {
        if self.sender_id != id {
            return false;
        }
        if signature.len() <= self.opcode as usize {
            return false;
        }
        let sig_message = signature[self.opcode as usize];
        if self.args.len() != sig_message.len() {
            return false;
        }
        for (index, arg) in self.args.iter().enumerate() {
            match sig_message[index] {
                ArgumentType::Int => assert!(arg.as_int().is_some()),
                ArgumentType::Uint => assert!(arg.as_uint().is_some()),
                ArgumentType::Fixed => assert!(arg.as_fixed().is_some()),
                ArgumentType::Str => assert!(arg.as_str().is_some()),
                ArgumentType::Object => assert!(arg.as_object().is_some()),
                ArgumentType::NewId => assert!(arg.as_new_id().is_some()),
                ArgumentType::Array => assert!(arg.as_array().is_some()),
                ArgumentType::Fd => assert!(arg.as_fd().is_some()),
            }
        }
        return true;
    }
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

#[derive(Debug)]
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
    PollError(String),
    #[error("unknown opcode `{0}`")]
    UnknownOpcode(u16),
    #[error("convert string failed `{0}`")]
    IntroStringError(#[from] IntoStringError),
    #[error("unix error `{0}`")]
    UnixError(#[from] Errno),
    #[error("try io error")]
    TryIoError,
    #[error("missing object for id `{0}`")]
    MissingObject(u32),
}

impl<T> From<PollSendError<T>> for WaylandError {
    fn from(err: PollSendError<T>) -> Self {
        WaylandError::PollError(err.to_string())
    }
}

/// Trait to be implemented on Wayland interface request types.
///
/// Although this trait can be implemented for foreign types,
/// it is not of any use if the concrete type that implements WaylandInterface is private.
pub trait RequestObject {
    type Interface: WaylandInterface;
    type ReturnType: WaylandInterface;
    /// Apply this request to `interface`.
    /// Returns optionally a new Wayland interface for a new object and a Wayland message.
    fn apply(self, self_id: u32, interface: &mut Self::Interface) -> (Self::ReturnType, Message);
    fn id(&self) -> u32;
}

pub trait Request {
    type Interface: WaylandInterface;

    fn apply(self, self_id: u32, interface: &mut Self::Interface) -> Message;
}

pub trait Event: Sized {
    type Interface: WaylandInterface;

    fn apply(
        message: Message,
        interface: &mut Self::Interface,
    ) -> Result<Processed<Self>, WaylandError>;
}

pub enum Processed<T> {
    Event(T),
    Destroyed(T),
    Destroy(u32, T),
    None,
}

pub trait WaylandInterface {
    fn event_signature() -> Signature;
    fn request_signature() -> Signature;
    fn interface() -> &'static str;
    fn version() -> u32;
    fn destroy(&mut self, self_id: u32) -> Option<Message> {
        None
    }
}

impl<T, R, D> Drop for WlObject<T, R, D>
where
    T: Sink<WlConnectionMessage, Error = WaylandError> + Unpin + Clone,
    R: Stream<Item = Message> + Unpin,
    D: WaylandInterface,
{
    fn drop(&mut self) {
        match self.data.destroy(self.id) {
            Some(message) => {
                block_on(self.request_tx.send(WlConnectionMessage::Destroy(self.id)))
                    .expect("failed to send destroy message");
                block_on(self.request_tx.send(WlConnectionMessage::Message(message)))
                    .expect("failed to send destroy message");
            }
            None => {}
        }
    }
}

pub struct WlObject<
    T: Sink<WlConnectionMessage, Error = WaylandError> + Unpin + Clone,
    R: Stream<Item = Message> + Unpin,
    D: WaylandInterface,
> {
    pub id_counter: Arc<RwLock<u32>>,
    pub id: u32,
    pub request_tx: T,
    pub message_rx: R,
    pub data: D,
}

impl<
        T: Sink<WlConnectionMessage, Error = WaylandError> + Unpin + Clone,
        R: Stream<Item = Message> + Unpin,
        D: WaylandInterface,
    > WlObject<T, R, D>
{
    pub async fn next_event<E: Event<Interface = D>>(&mut self) -> Result<Option<E>, WaylandError> {
        loop {
            match self.message_rx.next().await {
                Some(message) => match E::apply(message, &mut self.data)? {
                    Processed::Event(event) => return Ok(Some(event)),
                    Processed::Destroyed(event) => {
                        self.request_tx
                            .send(WlConnectionMessage::Destroy(self.id))
                            .await?;
                        return Ok(Some(event));
                    }
                    Processed::Destroy(id, event) => {
                        self.request_tx
                            .send(WlConnectionMessage::Destroy(id))
                            .await?;
                        return Ok(Some(event));
                    }
                    Processed::None => continue,
                },
                None => return Ok(None),
            };
        }
    }

    pub fn get_new_id(&self) -> Result<u32, PoisonError<RwLockWriteGuard<u32>>> {
        let mut guard = self.id_counter.write()?;
        *guard += 1;
        Ok(*guard)
    }

    pub async fn request<Req: Request<Interface = D>>(
        &mut self,
        request: Req,
    ) -> Result<(), WaylandError> {
        let message = request.apply(self.id, &mut self.data);
        assert!(
            message.is_valid(D::request_signature(), self.id),
            "message does not match signature"
        );
        self.request_tx
            .send(WlConnectionMessage::Message(message))
            .await?;
        Ok(())
    }

    pub async fn request_object<Req: RequestObject<Interface = D>>(
        &mut self,
        request: Req,
    ) -> Result<WlObject<T, ReceiverStream<Message>, Req::ReturnType>, WaylandError> {
        let id = request.id();
        let (return_value, request_message) = request.apply(self.id, &mut self.data);
        assert!(
            request_message.is_valid(D::request_signature(), self.id),
            "message does not match signature"
        );
        let (sender, receiver) = channel::<Message>(10);
        let sender_sink = Box::new(PollSender::new(sender).sink_err_into());
        let receiver_stream = ReceiverStream::new(receiver);

        let object = WlObject {
            id_counter: self.id_counter.clone(),
            id,
            request_tx: self.request_tx.clone(),
            message_rx: receiver_stream,
            data: return_value,
        };
        self.request_tx
            .send(WlConnectionMessage::Create(
                id,
                Req::ReturnType::event_signature(),
                sender_sink,
            ))
            .await?;
        self.request_tx
            .send(WlConnectionMessage::Message(request_message))
            .await?;
        Ok(object)
    }
}

pub const MAX_FDS_OUT: usize = 28;
pub const MAX_BYTES_OUT: usize = 4096;

pub struct WlSocket {
    inner: AsyncFd<UnixStream>,
    ancillary_buffer: Vec<u8>,
    buffer: [u8; MAX_BYTES_OUT],
    read_buffer: BytesMut,
    write_buffer: BytesMut,
    write_argbuffer: BytesMut,
    fds: VecDeque<RawFd>,
    header: Option<Header>,
    unprocessed_msg: Option<RawMessage>,
}

impl WlSocket {
    pub fn new(socket: UnixStream) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            inner: AsyncFd::new(socket)?,
            ancillary_buffer: cmsg_space!([RawFd; MAX_FDS_OUT]),
            buffer: [0u8; MAX_BYTES_OUT],
            read_buffer: BytesMut::new(),
            write_buffer: BytesMut::new(),
            write_argbuffer: BytesMut::new(),
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
                return Ok(None);
            }

            let object_id = self.read_buffer.get_u32_ne();
            let combined_val = self.read_buffer.get_u32_ne();
            let message_size = (combined_val >> 16) as u16;
            let opcode = (combined_val & 0x0000_FFFF) as u16;

            self.header = Some(Header {
                object_id,
                message_size,
                opcode,
            });

            self.decode_raw()
        }
    }

    fn decode(&mut self, map: &HashMap<u32, Signature>) -> Result<Option<Message>, WaylandError> {
        if let Some(mut raw_message) = self.unprocessed_msg.take() {
            let event_map = map.get(&raw_message.header.object_id).unwrap();
            let argument_list = *event_map.get(raw_message.header.opcode as usize).unwrap();
            if argument_list.len() > INLINE_ARGS {
                panic!("Too many arguments for message")
            }
            let mut args = SmallVec::new();
            let mut used_fds = VecDeque::new();
            for arg in argument_list {
                let argument = match arg {
                    ArgumentType::Int => Argument::Int(raw_message.args.get_i32_ne()),
                    ArgumentType::Uint => Argument::Uint(raw_message.args.get_u32_ne()),
                    ArgumentType::Fixed => Argument::Fixed(raw_message.args.get_i32_ne()),
                    ArgumentType::Str => {
                        let string_length = raw_message.args.get_u32_ne();
                        let string_bytes =
                            raw_message.args.copy_to_bytes((string_length - 1) as usize);
                        raw_message
                            .args
                            .advance(1 + ((4 - (string_length % 4)) % 4) as usize);
                        let string = CString::new(string_bytes.to_vec()).unwrap();
                        Argument::Str(Box::new(string))
                    }
                    ArgumentType::Object => Argument::Object(raw_message.args.get_u32_ne()),
                    ArgumentType::NewId => Argument::NewId(raw_message.args.get_u32_ne()),
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
            let result_message = Message {
                sender_id: raw_message.header.object_id,
                opcode: raw_message.header.opcode,
                args,
            };
            Ok(Some(result_message))
        } else {
            self.unprocessed_msg = self.decode_raw()?;
            //println!("decoded raw message {:?}", self.unprocessed_msg);
            if self.unprocessed_msg.is_none() {
                return Ok(None);
            }
            self.decode(map)
        }
    }

    fn encode(&mut self, msg: Message) -> Result<Vec<RawFd>, WaylandError> {
        let argument_bytes = &mut self.write_argbuffer;
        let mut fds = Vec::new();
        for arg in msg.args {
            match arg {
                Argument::Uint(val) => argument_bytes.put_u32_ne(val),
                Argument::Int(val) => argument_bytes.put_i32_ne(val),
                Argument::Fixed(val) => argument_bytes.put_i32_ne(val),
                Argument::Str(val) => {
                    let bytes = val.as_bytes_with_nul();
                    let len = bytes.len() as u32;
                    argument_bytes.put_u32_ne(len);
                    argument_bytes.put_slice(bytes);
                    let pad = 4 - (len % 4);
                    argument_bytes.put_bytes(0, pad as usize);
                }
                Argument::Object(val) => argument_bytes.put_u32_ne(val),
                Argument::NewId(val) => argument_bytes.put_u32_ne(val),
                Argument::Array(val) => todo!(),
                Argument::Fd(val) => fds.push(val),
            }
        }

        self.write_buffer.put_u32_ne(msg.sender_id);
        let val = (argument_bytes.len() + 8 << 16u16) as u32 | msg.opcode as u32;
        self.write_buffer.put_u32_ne(val);
        self.write_buffer.put_slice(argument_bytes);
        argument_bytes.clear();
        Ok(fds)
    }

    pub async fn read_message(
        &mut self,
        map: &HashMap<u32, Signature>,
    ) -> Result<Message, WaylandError> {
        loop {
            return match self.decode(map)? {
                Some(message) => Ok(message),
                None => {
                    let mut guard = self.inner.readable_mut().await?;
                    let bytes_read = match guard.try_io(|inner| {
                        let mut iov = [IoSliceMut::new(&mut self.buffer)];
                        let msg = socket::recvmsg::<()>(
                            inner.as_raw_fd(),
                            &mut iov[..],
                            Some(&mut self.ancillary_buffer),
                            socket::MsgFlags::MSG_DONTWAIT,
                        )?;
                        for cmsg in msg.cmsgs() {
                            match cmsg {
                                socket::ControlMessageOwned::ScmRights(fd) => {
                                    self.fds.extend(fd.iter())
                                }
                                _ => {} //ignore
                            }
                        }
                        Ok(msg.bytes)
                    }) {
                        Ok(result) => result?,
                        Err(_) => continue,
                    };
                    println!("read {} bytes", bytes_read);
                    if bytes_read == 0 {
                        return Err(WaylandError::TryIoError);
                    }
                    self.read_buffer.put_slice(&self.buffer[..bytes_read]);
                    continue;
                }
            };
        }
    }

    pub async fn write_message(&mut self, message: Message) -> Result<(), WaylandError> {
        println!("writing message {:?}", message);
        let fds = self.encode(message)?;
        let mut guard = self.inner.writable_mut().await?;
        match guard
            .try_io(|inner| {
                println!("{:?}", self.write_buffer.as_ref());
                let iov = [IoSlice::new(&self.write_buffer)];
                if !fds.is_empty() {
                    let cmsgs = [socket::ControlMessage::ScmRights(fds.as_slice())];
                    socket::sendmsg::<()>(
                        inner.as_raw_fd(),
                        &iov,
                        &cmsgs,
                        socket::MsgFlags::MSG_DONTWAIT,
                        None,
                    )?;
                } else {
                    socket::sendmsg::<()>(
                        inner.as_raw_fd(),
                        &iov,
                        &[],
                        socket::MsgFlags::MSG_DONTWAIT,
                        None,
                    )?;
                };
                Ok(())
            })
            .map_err(|_| WaylandError::TryIoError)
        {
            Ok(it) => println!("{:?}", it),
            Err(err) => return Err(err),
        };
        self.write_buffer.clear();
        println!("written message");
        Ok(())
    }
}
