use std::{env, ffi::CString, io, os::unix::prelude::RawFd, path::PathBuf};

use bytes::BytesMut;
use futures::{SinkExt, StreamExt};
use smallvec::{smallvec, SmallVec};
use thiserror::Error;
use tokio::net::UnixStream;
use tokio_util::codec::{Decoder, Encoder, Framed};
use wire::{Argument, Message};

pub mod connection;
pub mod wire;

mod tests {
    use crate::connection::WaylandConnection;

    #[tokio::test]
    async fn test_registry() {
        let mut conn = WaylandConnection::new().await.unwrap();
        let registry = conn.setup().await.unwrap();
        tokio::spawn(async move { conn.run().await });
    }
}
