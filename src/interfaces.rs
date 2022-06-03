use phf::phf_map;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::PollSender;

use crate::{
    connection::WlConnectionMessage,
    wire::{
        Argument, ArgumentType, Message, RequestWithReturn, WaylandError, WaylandInterface,
        WlObject,
    },
};

use smallvec::smallvec;

pub struct WlDisplay {}

pub enum DisplayEvent {}

pub struct GetRegistryRequest {}

impl RequestWithReturn for GetRegistryRequest {
    type Interface = WlDisplay;
    type ReturnType = WlRegistry;

    fn apply(
        self,
        self_id: u32,
        _interface: &mut Self::Interface,
        new_id: u32,
    ) -> (Option<Self::ReturnType>, Message) {
        let args = smallvec![Argument::NewId(new_id)];
        let message = Message {
            sender_id: self_id,
            opcode: 1,
            args,
        };
        (Some(WlRegistry {}), message)
    }
}

impl WaylandInterface for WlDisplay {
    type Event = DisplayEvent;

    fn process(&mut self, message: Message) -> Result<Option<Self::Event>, WaylandError> {
        todo!()
    }

    fn signature() -> &'static phf::Map<u16, &'static [ArgumentType]> {
        static ERROR: &'static [ArgumentType] =
            &[ArgumentType::Object, ArgumentType::Uint, ArgumentType::Str];
        static DELETE_ID: &'static [ArgumentType] = &[ArgumentType::Uint];
        static DISPLAY_EVENTS: phf::Map<u16, &[ArgumentType]> = phf_map! {
            0u16 => ERROR,
            1u16 => DELETE_ID,
        };
        return &DISPLAY_EVENTS;
    }
}

pub type Registry = WlObject<PollSender<Message>, ReceiverStream<Message>, WlRegistry>;

#[derive(Debug)]
pub enum RegistryEvent {
    Global(GlobalEvent),
    GlobalRemove(GlobalRemoveEvent),
}

#[derive(Debug)]
pub struct GlobalEvent {
    name: u32,
    interface: String,
    version: u32,
}

#[derive(Debug)]
pub struct GlobalRemoveEvent {
    name: u32,
}

pub struct WlRegistry {}

impl WaylandInterface for WlRegistry {
    type Event = RegistryEvent;

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

    fn signature() -> &'static phf::Map<u16, &'static [ArgumentType]> {
        static GLOBAL: &'static [ArgumentType] =
            &[ArgumentType::Uint, ArgumentType::Str, ArgumentType::Uint];
        static GLOBAL_REMOVE: &'static [ArgumentType] = &[ArgumentType::Uint];
        static REGISTRY_EVENTS: phf::Map<u16, &[ArgumentType]> = phf_map! {
            0u16 => GLOBAL,
            1u16 => GLOBAL_REMOVE,
        };
        return &REGISTRY_EVENTS;
    }
}
