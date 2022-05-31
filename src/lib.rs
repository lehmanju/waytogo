pub mod connection;
pub mod wire;

mod tests {
    use crate::connection::WaylandConnection;    

    #[tokio::test]
    async fn test_registry() {
        let mut conn = WaylandConnection::new().unwrap();
        let registry = conn.setup().await.unwrap();
        conn.run().await
    }
}
