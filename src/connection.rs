use std::{
    collections::HashMap,
    env, io,
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    task::Poll,
};

use async_trait::async_trait;
use futures::{ready, Sink, SinkExt, Stream, StreamExt};

use pin_project_lite::pin_project;
use smallvec::smallvec;
use tokio::select;
use tokio::{
    net::UnixStream,
    sync::mpsc::{channel, Receiver, Sender},
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::{codec::Framed, sync::PollSender};

use crate::wire::{Argument, Message, WaylandError, WaylandInterface, WaylandProtocol, WlObject};

pub struct WaylandConnection {
    framed: Framed<UnixStream, WaylandProtocol>,
    objects: HashMap<u32, Box<dyn Sink<Message, Error = WaylandError> + Unpin>>,
    id_counter: Arc<AtomicU32>,
    //TODO use channels without Arc because its single-threaded
    requests_rx: Receiver<Message>,
    requests_tx: Sender<Message>,
}

impl WaylandConnection {
    pub async fn new() -> io::Result<Self> {
        // create socket connection
        let xdg_dir = env::var_os("XDG_RUNTIME_DIR").unwrap();
        let mut path: PathBuf = xdg_dir.into();
        path.push("wayland-0");

        let socket = UnixStream::connect(path).await?;
        // convert to framed
        let framed = Framed::new(socket, WaylandProtocol {});
        let (tx, rx) = channel::<Message>(100);
        Ok(Self {
            framed,
            objects: HashMap::new(),
            id_counter: Arc::new(AtomicU32::new(2)),
            requests_rx: rx,
            requests_tx: tx,
        })
    }
    async fn setup(
        &mut self,
    ) -> Result<WlObject<PollSender<Message>, ReceiverStream<Message>, WlRegistry>, WaylandError>
    {
        //send initial request
        let new_id = self.id_counter.fetch_add(1, Ordering::Relaxed);
        let message = Message {
            sender_id: 1, // wl_display
            opcode: 1,    // get registry
            args: smallvec![
                Argument::NewId(new_id), // id of the created registry
            ],
        };
        self.framed.send(message).await?;
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
        Ok(registry)
    }
    async fn run(mut self) {
        let (mut writer, mut reader) = self.framed.split();
        loop {
            tokio::select! {
                incoming = reader.next() => {
                    let message = incoming.unwrap().unwrap();
                    let sink = self.objects.get_mut(&message.sender_id).unwrap();
                    sink.send(message).await.unwrap();
                }
                outgoing = self.requests_rx.recv() => {
                    let message = outgoing.unwrap();
                    writer.send(message).await.unwrap();
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

impl WaylandInterface for WlRegistry {
    type Event = RegistryEvent;
    type Request = RegistryRequest;

    fn process(&mut self, message: Message) -> Result<Option<Self::Event>, WaylandError> {
        let Message {
            sender_id: _,
            opcode,
            mut args,
        } = message;
        match opcode {
            0 => {
                let name: u32;
                let interface: String;
                let version: u32;
                if let Argument::Uint(value) = args.remove(0) {
                    name = value;
                } else {
                    return Err(WaylandError::ParseError);
                }
                if let Argument::Str(value) = args.remove(0) {
                    interface = value.into_string()?;
                } else {
                    return Err(WaylandError::ParseError);
                }
                if let Argument::Uint(value) = args.remove(0) {
                    version = value;
                } else {
                    return Err(WaylandError::ParseError);
                }
                Ok(Some(RegistryEvent::Global(GlobalEvent {
                    name,
                    interface,
                    version,
                })))
            }
            1 => {
                let name: u32;
                if let Argument::Uint(value) = args.remove(0) {
                    name = value;
                } else {
                    return Err(WaylandError::ParseError);
                }

                Ok(Some(RegistryEvent::GlobalRemove(GlobalRemoveEvent {
                    name,
                })))
            }
            _ => Err(WaylandError::UnknownOpcode(opcode)),
        }
    }

    fn request(&mut self, _request: Self::Request) -> Result<Option<Message>, WaylandError> {
        Ok(None)
    }
}
