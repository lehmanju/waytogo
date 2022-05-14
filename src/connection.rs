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
use futures::{ready, Sink, SinkExt, Stream};

use pin_project_lite::pin_project;
use smallvec::smallvec;
use tokio::{
    net::UnixStream,
    sync::mpsc::{channel, Receiver, Sender},
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::{codec::Framed, sync::PollSender};

use crate::wire::{Argument, Message, WaylandError, WaylandProtocol};

pub struct WaylandConnection {
    framed: Framed<UnixStream, WaylandProtocol>,
    objects: HashMap<u32, Box<dyn Sink<Message, Error = WaylandError>>>,
    id_counter: Arc<AtomicU32>,
    //TODO use channels without Arc because its single-threaded
    requests_rx: Receiver<Request>,
    requests_tx: Sender<Request>,
}

pub enum Request {
    NewObject(NewObjectReq),
}

pub struct NewObjectReq {
    id: u32,
    sink: Box<dyn Sink<Message, Error = WaylandError> + Send>,
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
        let (tx, rx) = channel::<Request>(100);
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
    ) -> Result<WlObject<PollSender<Request>, ReceiverStream<Message>, Registry>, WaylandError>
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
            data: Registry {},
        };
        self.objects.insert(new_id, Box::new(sender_sink));
        Ok(registry)
    }
    async fn run(mut self) {
        //loop select
        //check framed receive
        //check request receive (send)
    }
}

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

pub struct Registry {}

impl WaylandInterface for Registry {
    type Event = RegistryEvent;
    type Request = RegistryRequest;

    fn process(
        &mut self,
        message: Message,
    ) -> Result<(Option<Self::Event>, Option<Request>), WaylandError> {
        todo!()
    }

    fn request(&mut self, reguest: Self::Request) -> Result<Option<Request>, WaylandError> {
        todo!()
    }
}

pub trait WaylandInterface {
    type Event;
    type Request;
    fn process(
        &mut self,
        message: Message,
    ) -> Result<(Option<Self::Event>, Option<Request>), WaylandError>;
    fn request(&mut self, reguest: Self::Request) -> Result<Option<Request>, WaylandError>;
}

pin_project! {
    #[project=Proj]
    pub struct WlObject<T: Sink<Request>, R: Stream<Item = Message>, D: WaylandInterface> {
        id_counter: Arc<AtomicU32>,
        id: u32,
        #[pin]
        request_tx: T,
        #[pin]
        message_rx: R,
        data: D,
    }
}

impl<T, R, D> WaylandInterface for WlObject<T, R, D>
where
    T: Sink<Request>,
    R: Stream<Item = Message>,
    D: WaylandInterface,
{
    type Event = D::Event;

    fn process(
        &mut self,
        message: Message,
    ) -> Result<(Option<Self::Event>, Option<Request>), WaylandError> {
        self.data.process(message)
    }

    type Request = D::Request;

    fn request(&mut self, reguest: Self::Request) -> Result<Option<Request>, WaylandError> {
        todo!()
    }
}

impl<T, R, D> Stream for WlObject<T, R, D>
where
    T: Sink<Request>,
    R: Stream<Item = Message>,
    D: WaylandInterface,
{
    type Item = Result<D::Event, WaylandError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let Proj {
            id_counter,
            id,
            request_tx,
            mut message_rx,
            mut data,
        } = self.project();
        match message_rx.as_mut().poll_next(cx) {
            Poll::Ready(value) => {
                match value {
                    Some(message) => {
                        let (result, response) = data.process(message)?;
                        // return result if Some
                        // send response to queue
                    }
                    None => todo!(),
                }
                todo!()
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
