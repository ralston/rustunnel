use rustunnel_protocol::{ControlFrame, encode_frame, decode_frame};

#[tokio::main]
async fn main() {
    println!("rustunnel-client starting...");

    let frame = ControlFrame::Auth {
        token: "secret".to_string(),
        client_version: "0.1.0".to_string(),
    };
    let encoded = encode_frame(&frame);
    let decoded = decode_frame(&encoded).expect("round-trip decode");
    println!("self-check: {:?}", decoded);
}
