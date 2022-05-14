use std::{env, ffi::CString, io, os::unix::prelude::RawFd, path::PathBuf};

use bytes::BytesMut;
use futures::{SinkExt, StreamExt};
use smallvec::{smallvec, SmallVec};
use thiserror::Error;
use tokio::net::UnixStream;
use tokio_util::codec::{Decoder, Encoder, Framed};
use wire::{Argument, Message, WaylandProtocol};

mod wire;
mod connection;

#[macro_use]
extern crate wayland_sys;

struct Registry {
    id: u32,
    name: u32,
}

async fn wayland_connection() -> io::Result<UnixStream> {
    // create socket connection
    let xdg_dir = env::var_os("XDG_RUNTIME_DIR").unwrap();
    let mut path: PathBuf = xdg_dir.into();
    path.push("wayland-0");

    let socket = UnixStream::connect(path).await?;
    // convert to framed
    let mut framed = Framed::new(socket, WaylandProtocol {});
    // create wl_display
    let message = Message {
        sender_id: 1, // wl_display
        opcode: 1,    // get registry
        args: smallvec![
            Argument::NewId(2), // id of the created registry
        ],
    };
    framed.send(message).await.unwrap();
    let response = framed.next().await.unwrap().unwrap();

    let name = if let Argument::Uint(name) = response.args[0] {
        name
    } else {
        panic!()
    };
    assert!(response.opcode == 0);
    let registry = Registry {
        id: response.sender_id,
        name,
    };
    todo!();

    // bind wl_global
    // loop over global events
    Ok(socket)
}
