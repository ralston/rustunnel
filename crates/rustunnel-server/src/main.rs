use rustunnel_server::core::{ControlMessage, TunnelCore};

#[tokio::main]
async fn main() {

    println!("rustunnel-server starting…");

    let core = TunnelCore::new([20000, 20099], 10);
    let addr = "127.0.0.1:0".parse().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel::<ControlMessage>(16);
    let session_id = core.register_session(addr, "demo".to_string(), tx);
    println!("demo session registered: {session_id}");
}
