use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, CString},
    hash::Hash,
    marker::PhantomData,
};

use phf::phf_map;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::PollSender;

use crate::wire::{
    Argument, ArgumentType, Event, Id, LookupId, Message, NewId, Processed, RequestObject,
    Signature, WaylandError, WaylandInterface, WlObject,
};

use smallvec::smallvec;

pub struct WlDisplay {}

pub struct ErrorEvent {
    pub object_id: u32,
    pub code: u32,
    pub message: String,
}

pub struct DeleteIdEvent {
    pub id: u32,
}

pub struct GetRegistryRequest {
    pub registry: NewId,
}

pub struct SyncRequest {
    pub callback: u32,
}

/*

impl RequestObject for SyncRequest {
    type Interface = WlDisplay;
    type ReturnType = WlCallback;

    fn apply(self, self_id: u32, interface: &mut Self::Interface) -> (Self::ReturnType, Message) {
        todo!()
    }

    fn id(&self) -> u32 {
        todo!()
    }
}

*/

impl RequestObject for GetRegistryRequest {
    type Interface = WlDisplay;
    type ReturnType = WlRegistry;

    fn apply(self, self_id: Id, _interface: &mut Self::Interface) -> (Self::ReturnType, Message) {
        let args = smallvec![Argument::NewId(self.registry)];
        let message = Message {
            sender_id: self_id,
            opcode: 1,
            args,
        };
        (
            WlRegistry {
                name_id_map: HashMap::new(),
                available_names: HashSet::new(),
            },
            message,
        )
    }

    fn id(&self) -> &NewId {
        &self.registry
    }
}

impl Event for ErrorEvent {
    type Interface = WlDisplay;

    fn apply(
        mut message: Message,
        interface: &mut Self::Interface,
    ) -> Result<Processed<Self>, WaylandError> {
        let mut drain_iter = message.args.drain(..);
        let object_id = drain_iter.next().unwrap().into_object().unwrap();
        let code = drain_iter.next().unwrap().into_uint().unwrap();
        let message = drain_iter
            .next()
            .unwrap()
            .into_str()
            .unwrap()
            .into_string()
            .unwrap();

        Ok(Processed::Event(Self {
            object_id,
            code,
            message,
        }))
    }
}

impl Event for DeleteIdEvent {
    type Interface = WlDisplay;

    fn apply(
        mut message: Message,
        interface: &mut Self::Interface,
    ) -> Result<Processed<Self>, WaylandError> {
        let mut drain_iter = message.args.drain(..);
        let id = drain_iter.next().unwrap().into_uint().unwrap();
        Ok(Processed::Event(DeleteIdEvent { id }))
    }
}

impl WaylandInterface for WlDisplay {
    fn event_signature() -> Signature {
        const ERROR: &[ArgumentType] =
            &[ArgumentType::Object, ArgumentType::Uint, ArgumentType::Str];
        const DELETE_ID: &[ArgumentType] = &[ArgumentType::Uint];
        const DISPLAY_EVENTS: Signature = &[ERROR, DELETE_ID];
        &DISPLAY_EVENTS
    }

    fn request_signature() -> Signature {
        const SYNC: &[ArgumentType] = &[ArgumentType::NewId];
        const GET_REGISTRY: &[ArgumentType] = &[ArgumentType::NewId];
        &[SYNC, GET_REGISTRY]
    }

    fn interface() -> &'static str {
        todo!()
    }

    fn version() -> u32 {
        todo!()
    }
}

// Registry

#[derive(Debug)]
pub enum RegistryEvent {
    Global(GlobalEvent),
    GlobalRemove(GlobalRemoveEvent),
}

#[derive(Debug)]
pub struct GlobalEvent {
    pub name: u32,
    pub interface: String,
    pub version: u32,
}

#[derive(Debug)]
pub struct GlobalRemoveEvent {
    pub name: u32,
}

pub struct BindRequest<T> {
    pub interface: T,
    pub name: u32,
    pub id: NewId,
}

impl<T: WaylandInterface> RequestObject for BindRequest<T> {
    type Interface = WlRegistry;
    type ReturnType = T;

    fn apply(self, self_id: Id, interface: &mut Self::Interface) -> (Self::ReturnType, Message) {
        if interface.available_names.contains(&self.name) {
            interface
                .name_id_map
                .insert(self.name, self.id.get_lookup());
            let message = Message {
                sender_id: self_id,
                opcode: 0,
                args: smallvec![
                    Argument::Uint(self.name),
                    Argument::Str(Box::new(CString::new(T::interface()).unwrap())),
                    Argument::Uint(T::version()),
                    Argument::NewId(self.id)
                ],
            };
            return (self.interface, message);
        }
        panic!("Invalid object name")
    }

    fn id(&self) -> &NewId {
        &self.id
    }
}

impl Event for GlobalEvent {
    type Interface = WlRegistry;

    fn apply(
        mut message: Message,
        interface: &mut Self::Interface,
    ) -> Result<Processed<Self>, WaylandError> {
        let mut drain_iter = message.args.drain(..);
        let name = drain_iter.next().unwrap().into_uint().unwrap();
        let interface_name: String = drain_iter
            .next()
            .unwrap()
            .into_str()
            .unwrap()
            .into_string()?;
        let version = drain_iter.next().unwrap().into_uint().unwrap();

        interface.available_names.insert(name);

        Ok(Processed::Event(GlobalEvent {
            name,
            interface: interface_name,
            version,
        }))
    }
}

impl Event for GlobalRemoveEvent {
    type Interface = WlRegistry;

    fn apply(
        mut message: Message,
        interface: &mut Self::Interface,
    ) -> Result<Processed<Self>, WaylandError> {
        let mut drain_iter = message.args.drain(..);
        let name = drain_iter.next().unwrap().into_uint().unwrap();
        let is_mapped = interface.name_id_map.contains_key(&name);
        interface.available_names.remove(&name);
        if is_mapped {
            let id = interface.name_id_map.remove(&name).unwrap();
            return Ok(Processed::Destroy(id, GlobalRemoveEvent { name }));
        }

        Ok(Processed::Event(GlobalRemoveEvent { name }))
    }
}

impl Event for RegistryEvent {
    type Interface = WlRegistry;

    fn apply(
        message: Message,
        interface: &mut Self::Interface,
    ) -> Result<Processed<Self>, WaylandError> {
        match message.opcode {
            0 => GlobalEvent::apply(message, interface).map(|val| match val {
                Processed::Event(event) => Processed::Event(Self::Global(event)),
                Processed::Destroyed(event) => Processed::Destroyed(Self::Global(event)),
                Processed::Destroy(id, event) => Processed::Destroy(id, Self::Global(event)),
                Processed::None => Processed::None,
            }),
            1 => GlobalRemoveEvent::apply(message, interface).map(|val| match val {
                Processed::Event(event) => Processed::Event(Self::GlobalRemove(event)),
                Processed::Destroyed(event) => Processed::Destroyed(Self::GlobalRemove(event)),
                Processed::Destroy(id, event) => Processed::Destroy(id, Self::GlobalRemove(event)),
                Processed::None => Processed::None,
            }),
            _ => Err(WaylandError::UnknownOpcode(message.opcode)),
        }
    }
}

pub struct WlRegistry {
    name_id_map: HashMap<u32, LookupId>,
    available_names: HashSet<u32>,
}

impl WaylandInterface for WlRegistry {
    fn event_signature() -> Signature {
        const GLOBAL: &[ArgumentType] =
            &[ArgumentType::Uint, ArgumentType::Str, ArgumentType::Uint];
        const GLOBAL_REMOVE: &[ArgumentType] = &[ArgumentType::Uint];
        const REGISTRY_EVENTS: Signature = &[GLOBAL, GLOBAL_REMOVE];
        &REGISTRY_EVENTS
    }

    fn request_signature() -> Signature {
        const BIND: &[ArgumentType] = &[
            ArgumentType::Uint,
            ArgumentType::Str,
            ArgumentType::Uint,
            ArgumentType::NewId,
        ];
        &[BIND]
    }

    fn interface() -> &'static str {
        todo!()
    }

    fn version() -> u32 {
        todo!()
    }
}

// Shm

pub struct WlShm;

impl WaylandInterface for WlShm {
    fn event_signature() -> Signature {
        const FORMAT: &[ArgumentType] = &[ArgumentType::Uint];
        &[FORMAT]
    }
    fn request_signature() -> Signature {
        &[]
    }

    fn interface() -> &'static str {
        "wl_shm"
    }

    fn version() -> u32 {
        1
    }
}
