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

#[cfg(test)]
mod tests {
    use crate::{
        connection::WaylandConnection,
        interfaces::{BindRequest, GetRegistryRequest, WlDisplay, WlShm},
        wire::WaylandInterface,
    };

    #[tokio::test]
    async fn test_registry() {
        let mut conn = WaylandConnection::new().unwrap();
        let mut display = conn.setup(WlDisplay {}).await;
        tokio::spawn(conn.run());
        let new_id = display.get_new_id();
        let get_registry = GetRegistryRequest { registry: new_id };
        let mut registry = display.request_object(get_registry).await.unwrap();
        loop {
            match registry.next_event().await.unwrap() {
                Some(registry_event) => {
                    println!("Received registry event: {:?}", registry_event);
                    match registry_event {
                        crate::interfaces::RegistryEvent::Global(global) => {
                            if global.interface == "wl_shm" {
                                let new_id = display.get_new_id();
                                let bind = BindRequest {
                                    interface: WlShm {},
                                    name: global.name,
                                    id: new_id,
                                };
                                println!("got compositor object");
                                let shm = registry.request_object(bind).await.unwrap();
                                println!("got shm");
                                //break;
                            }
                        }
                        crate::interfaces::RegistryEvent::GlobalRemove(global_remove) => todo!(),
                    }
                }
                None => break,
            }
        }
    }
}
