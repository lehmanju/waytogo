use std::{collections::HashMap, env, io, os::unix::net::UnixStream, path::PathBuf};

use futures::{sink::SinkMapErr, Sink, SinkExt};

use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::{PollSendError, PollSender};

use crate::wire::{
    Id, IdRegistry, LookupId, Message, Signature, WaylandError, WaylandInterface, WlObject,
    WlSocket,
};
use std::fmt::Debug;

pub struct WaylandConnection {
    socket: WlSocket,
    objects: HashMap<LookupId, Box<dyn WlSink>>,
    interfaces: HashMap<LookupId, Signature>,
    id_counter: IdRegistry,
    requests_rx: Receiver<WlConnectionMessage>,
    requests_tx: Sender<WlConnectionMessage>,
    initial: Option<Id>,
}

#[derive(Debug)]
pub enum WlConnectionMessage {
    Create(LookupId, Signature, Box<dyn WlSink>),
    Destroy(LookupId),
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
        let (idreg, initial) = IdRegistry::new();
        let stream = UnixStream::connect(path)?;
        let socket = WlSocket::new(stream, idreg.clone())?;
        let (tx, rx) = channel::<WlConnectionMessage>(100);
        Ok(Self {
            socket,
            objects: HashMap::new(),
            interfaces: HashMap::new(),
            id_counter: idreg,
            requests_rx: rx,
            requests_tx: tx,
            initial: Some(initial),
        })
    }
    pub async fn setup<D: WaylandInterface>(
        &mut self,
        interface: D,
    ) -> WlObject<
        SinkMapErr<
            PollSender<WlConnectionMessage>,
            fn(PollSendError<WlConnectionMessage>) -> WaylandError,
        >,
        ReceiverStream<Message>,
        D,
    > {
        if self.initial.is_none() {
            panic!("setup can only be called once")
        }
        let id = self.initial.take().unwrap();
        let lookup = id.get_lookup();
        let (sender, receiver) = channel::<Message>(10);
        let sender_sink = PollSender::new(sender);
        let receiver_stream = ReceiverStream::new(receiver);
        let request_sink = PollSender::new(self.requests_tx.clone()).sink_map_err(
            Into::<WaylandError>::into as fn(PollSendError<WlConnectionMessage>) -> WaylandError,
        );
        let signature = D::event_signature();
        let registry = WlObject::new(id, request_sink, receiver_stream, interface);
        self.objects
            .insert(lookup.clone(), Box::new(sender_sink.sink_err_into()));
        self.interfaces.insert(lookup, signature);
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
        if incoming.is_err() {
            println!("error message read");
        }
        let message = incoming.unwrap();
        match self.objects.remove(&message.sender_id.get_lookup()) {
            Some(mut sink) => {
                let id = message.sender_id.get_lookup();
                if sink.send(message).await.is_ok() {
                    self.objects.insert(id, sink);
                }
            }
            None => eprintln!("Missing object for id {:?}", message.sender_id),
        }
    }

    async fn write(&mut self, outgoing: Option<WlConnectionMessage>) {
        match outgoing.unwrap() {
            WlConnectionMessage::Create(id, signature, sink) => {
                self.objects.insert(id.clone(), Box::new(sink));
                self.interfaces.insert(id, signature);
            }
            WlConnectionMessage::Destroy(id) => {
                self.objects.remove(&id);
                self.interfaces.remove(&id);
            }
            WlConnectionMessage::Message(message) => {
                self.socket.write_message(message).await.unwrap();
            }
        }
    }
}
