use std::{
    collections::HashMap,
    env, io,
    os::unix::{net::UnixStream, prelude::AsRawFd},
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
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
use tokio_util::{codec::Framed, sync::PollSender};

use crate::wire::{
    Argument, ArgumentType, Message, RawMessage, WaylandError, WaylandInterface, WlObject, WlSocket,
};

pub struct WaylandConnection {
    socket: WlSocket,
    objects: HashMap<u32, Box<dyn Sink<Message, Error = WaylandError> + Unpin + Send>>,
    interfaces: HashMap<u32, &'static phf::Map<u16, &'static [ArgumentType]>>,
    id_counter: Arc<AtomicU32>,
    //TODO use channels without Arc because its single-threaded
    requests_rx: Receiver<Message>,
    requests_tx: Sender<Message>,
}

impl WaylandConnection {
    pub fn new() -> io::Result<Self> {
        // create socket connection
        let xdg_dir = env::var_os("XDG_RUNTIME_DIR").unwrap();
        let wayland_display = env::var_os("WAYLAND_DISPLAY").unwrap();
        let mut path: PathBuf = xdg_dir.into();
        //path.push(wayland_display);
        //path.push("wldbg-wayland-0");
        path.push("wayland-0");
        dbg!(&path);
        let stream = UnixStream::connect(path)?;
        let socket = WlSocket::new(stream)?;
        let (tx, rx) = channel::<Message>(100);
        Ok(Self {
            socket,
            objects: HashMap::new(),
            interfaces: HashMap::new(),
            id_counter: Arc::new(AtomicU32::new(2)),
            requests_rx: rx,
            requests_tx: tx,
        })
    }
    pub async fn setup(&mut self) -> Result<Registry, WaylandError> {
        //send initial request
        let new_id = self.id_counter.fetch_add(1, Ordering::Relaxed);
        let message = Message {
            sender_id: 1, // wl_display
            opcode: 1,    // get registry
            args: smallvec![
                Argument::NewId(2), // id of the created registry
            ],
        };
        self.socket.write_message(message).await?;
        let (sender, receiver) = channel::<Message>(10);
        let sender_sink = PollSender::new(sender).sink_err_into();
        let receiver_stream = ReceiverStream::new(receiver);
        let request_sink = PollSender::new(self.requests_tx.clone());
        let registry = WlObject {
            id_counter: self.id_counter.clone(),
            id: new_id,
            request_tx: request_sink,
            message_rx: receiver_stream,
            data: WlRegistry {},
        };
        self.objects.insert(new_id, Box::new(sender_sink));
        self.interfaces.insert(new_id, &REGISTRY_EVENTS);
        Ok(registry)
    }
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                incoming = self.socket.read_message(&self.interfaces) => {
                    let message = incoming.unwrap();
                    let sink = self.objects.get_mut(&message.sender_id).unwrap();
                    sink.send(message).await.unwrap();
                }
                outgoing = self.requests_rx.recv() => {
                    let message = outgoing.unwrap();
                    self.socket.write_message(message).await.unwrap();
                }
            }
        }
        //loop select
        //check framed receive
        //check request receive (send)
    }
}
pub type Registry = WlObject<PollSender<Message>, ReceiverStream<Message>, WlRegistry>;

pub enum RegistryEvent {
    Global(GlobalEvent),
    GlobalRemove(GlobalRemoveEvent),
}

pub struct GlobalEvent {
    name: u32,
    interface: String,
    version: u32,
}

pub struct GlobalRemoveEvent {
    name: u32,
}

pub enum RegistryRequest {}

pub struct WlRegistry {}

static GLOBAL: &'static [ArgumentType] =
    &[ArgumentType::Uint, ArgumentType::Str, ArgumentType::Uint];
static GLOBAL_REMOVE: &'static [ArgumentType] = &[ArgumentType::Uint];
static REGISTRY_EVENTS: phf::Map<u16, &[ArgumentType]> = phf_map! {
    0u16 => GLOBAL,
    1u16 => GLOBAL_REMOVE,
};

impl WaylandInterface for WlRegistry {
    type Event = RegistryEvent;
    type Request = RegistryRequest;

    fn process(&mut self, mut message: Message) -> Result<Option<Self::Event>, WaylandError> {
        match message.opcode {
            0 => {
                let name = message.args.remove(0).into_uint().unwrap();
                let interface: String = message.args.remove(0).into_str().unwrap().into_string()?;
                let version = message.args.remove(0).into_uint().unwrap();
                Ok(Some(RegistryEvent::Global(GlobalEvent {
                    name,
                    interface,
                    version,
                })))
            }
            1 => {
                let name = message.args.pop().unwrap().into_uint().unwrap();

                Ok(Some(RegistryEvent::GlobalRemove(GlobalRemoveEvent {
                    name,
                })))
            }
            _ => Err(WaylandError::UnknownOpcode(message.opcode)),
        }
    }

    fn request(&mut self, _request: Self::Request) -> Result<Option<Message>, WaylandError> {
        Ok(None)
    }
}
