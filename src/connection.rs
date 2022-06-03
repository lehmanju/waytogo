use std::{
    collections::HashMap,
    env, io,
    os::unix::{net::UnixStream, prelude::AsRawFd},
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, RwLock,
    },
    task::Poll,
};

use async_trait::async_trait;
use futures::{ready, Sink, SinkExt, Stream, StreamExt};
use phf::phf_map;
use pin_project_lite::pin_project;
use smallvec::smallvec;
use tokio::select;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::{
    codec::Framed,
    sync::{PollSendError, PollSender},
};

use crate::{
    interfaces::{self, Registry},
    wire::{ArgumentType, Message, Signature, WaylandError, WaylandInterface, WlObject, WlSocket},
};
use std::fmt::Debug;

pub struct WaylandConnection {
    socket: WlSocket,
    objects: HashMap<u32, Box<dyn Sink<Message, Error = WaylandError> + Unpin + Send>>,
    interfaces: HashMap<u32, Signature>,
    id_counter: Arc<RwLock<u32>>,
    requests_rx: Receiver<WlConnectionMessage>,
    requests_tx: Sender<WlConnectionMessage>,
}

#[derive(Debug)]
pub enum WlConnectionMessage {
    Create(u32, Signature, Box<dyn WlSink>),
    Destroy(u32),
    Message(Message),
}

pub trait WlSink: Sink<Message, Error = WaylandError> + Unpin + Send + Debug {}

impl<T: Sink<Message, Error = WaylandError> + Unpin + Send + Debug> WlSink for T {}

impl WaylandConnection {
    pub fn new() -> io::Result<Self> {
        // create socket connection
        let xdg_dir = env::var_os("XDG_RUNTIME_DIR").unwrap();
        let wayland_display = env::var_os("WAYLAND_DISPLAY").unwrap();
        let mut path: PathBuf = xdg_dir.into();
        path.push(wayland_display);
        //path.push("wldbg-wayland-0");
        //path.push("wayland-0");
        dbg!(&path);
        let stream = UnixStream::connect(path)?;
        let socket = WlSocket::new(stream)?;
        let (tx, rx) = channel::<WlConnectionMessage>(100);
        Ok(Self {
            socket,
            objects: HashMap::new(),
            interfaces: HashMap::new(),
            id_counter: Arc::new(RwLock::new(1)),
            requests_rx: rx,
            requests_tx: tx,
        })
    }
    pub async fn setup<D: WaylandInterface>(
        &mut self,
        interface: D,
    ) -> WlObject<PollSender<WlConnectionMessage>, ReceiverStream<Message>, D> {
        if *self.id_counter.try_read().unwrap() != 1u32 {
            panic!("setup can only be called once")
        }
        let (sender, receiver) = channel::<Message>(10);
        let sender_sink = PollSender::new(sender);
        let receiver_stream = ReceiverStream::new(receiver);
        let request_sink = PollSender::new(self.requests_tx.clone());
        let signature = D::signature();
        let registry = WlObject {
            id_counter: self.id_counter.clone(),
            id: 1u32,
            request_tx: request_sink,
            message_rx: receiver_stream,
            data: interface,
        };
        self.objects
            .insert(1u32, Box::new(sender_sink.sink_err_into()));
        self.interfaces.insert(1u32, signature);
        registry
    }
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                incoming = self.socket.read_message(&self.interfaces) => {
                    self.read(incoming).await
                }
                outgoing = self.requests_rx.recv() => {
                    self.write(outgoing).await
                }
            }
        }
    }

    async fn read(&mut self, incoming: Result<Message, WaylandError>) {
        let message = incoming.unwrap();
        let mut sink = self.objects.remove(&message.sender_id).unwrap();
        sink.send(message).await.unwrap();
    }

    async fn write(&mut self, outgoing: Option<WlConnectionMessage>) {
        match outgoing.unwrap() {
            WlConnectionMessage::Create(id, signature, sink) => todo!(),
            WlConnectionMessage::Destroy(sink) => todo!(),
            WlConnectionMessage::Message(message) => {
                self.socket.write_message(message).await.unwrap()
            }
        }
    }
}
