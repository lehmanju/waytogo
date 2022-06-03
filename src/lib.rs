use bytes::{Buf, BufMut};

pub mod connection;
pub mod interfaces;
pub mod wire;

pub trait BufMutExt: BufMut {
    fn put_u32_ne(&mut self, val: u32) {
        self.put_slice(&val.to_ne_bytes())
    }
    fn put_i32_ne(&mut self, val: i32) {
        self.put_slice(&val.to_ne_bytes())
    }
}

impl<T> BufMutExt for T where T: BufMut {}

pub trait BufExt: Buf {
    fn get_u32_ne(&mut self) -> u32 {
        if cfg!(target_endian = "big") {
            self.get_u32()
        } else {
            self.get_u32_le()
        }
    }
    fn get_i32_ne(&mut self) -> i32 {
        if cfg!(target_endian = "big") {
            self.get_i32()
        } else {
            self.get_i32_le()
        }
    }
}

impl<T> BufExt for T where T: Buf {}

mod tests {
    use bytes::{Buf, BufMut, BytesMut};

    use crate::{
        connection::WaylandConnection,
        interfaces::{GetRegistryRequest, WlDisplay},
    };

    #[tokio::test]
    async fn test_registry() {
        let mut conn = WaylandConnection::new().unwrap();
        let mut display = conn.setup(WlDisplay {}).await;
        tokio::spawn(conn.run());
        let get_registry = GetRegistryRequest {};
        let mut registry = display.send_request(get_registry).await.unwrap().unwrap();
        loop {
            match registry.next_message().await.unwrap() {
                Some(registry_event) => {
                    println!("Received registry event: {:?}", registry_event)
                }
                None => break,
            }
        }
    }

    #[test]
    fn test_shift() {
        println!("result: {}", ((12u32 << 16) | u32::from(1u16)));
        let mut bytes = BytesMut::new();
        bytes.put_u16(12u16);
        bytes.put_u16(1u16);
        println!("bytes: {}", bytes.get_u32());
    }
}
