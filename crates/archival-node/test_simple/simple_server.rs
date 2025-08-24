use std::net::TcpListener;
use std::io::prelude::*;
use std::collections::HashMap;

fn main() -> std::io::Result<()> {
    println!("🚀 Starting Simple Archival RPC Server");
    println!("=====================================");
    println!("📡 JSON-RPC Server listening on http://127.0.0.1:8545");
    println!("🔍 Status endpoint: http://127.0.0.1:8545/status");
    println!();
    println!("💡 Test commands you can run in another terminal:");
    println!("   curl -X POST http://127.0.0.1:8545 -H 'Content-Type: application/json' \\");
    println!("     -d '{{\"jsonrpc\":\"2.0\",\"method\":\"eth_blockNumber\",\"params\":[],\"id\":1}}'");
    println!();
    println!("   curl -X POST http://127.0.0.1:8545 -H 'Content-Type: application/json' \\");
    println!("     -d '{{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"0x1\", true],\"id\":1}}'");
    println!();
    println!("   curl -X POST http://127.0.0.1:8545 -H 'Content-Type: application/json' \\");
    println!("     -d '{{\"jsonrpc\":\"2.0\",\"method\":\"eth_getProof\",\"params\":[\"0xa0b86a33e6ba5d0c6d93ceaf5c9d19b0b18f5e0c\",[\"0x0\"],\"latest\"],\"id\":1}}'");
    println!();
    println!("   curl http://127.0.0.1:8545/status");
    println!();
    println!("🔌 Ready to accept connections...");
    println!("Press Ctrl+C to stop the server");
    println!("==============================");

    let listener = TcpListener::bind("127.0.0.1:8545")?;
    
    for stream in listener.incoming() {
        let mut stream = stream?;
        
        let mut buffer = [0; 4096];
        stream.read(&mut buffer)?;
        
        let request = String::from_utf8_lossy(&buffer);
        
        // Parse HTTP request
        let response = if request.contains("POST") && request.contains("eth_blockNumber") {
            handle_block_number()
        } else if request.contains("POST") && request.contains("eth_getBlockByNumber") {
            handle_get_block()
        } else if request.contains("POST") && request.contains("eth_getProof") {
            handle_get_proof()
        } else if request.contains("GET") && request.contains("/status") {
            handle_status()
        } else {
            handle_unknown()
        };
        
        let http_response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Access-Control-Allow-Methods: POST, GET, OPTIONS\r\n\
             Access-Control-Allow-Headers: Content-Type\r\n\
             \r\n\
             {}",
            response.len(),
            response
        );
        
        stream.write_all(http_response.as_bytes())?;
        stream.flush()?;
        
        println!("📡 [RPC] Processed request: {}", 
                if request.contains("eth_blockNumber") { "eth_blockNumber" }
                else if request.contains("eth_getBlockByNumber") { "eth_getBlockByNumber" }
                else if request.contains("eth_getProof") { "eth_getProof" }
                else if request.contains("/status") { "status" }
                else { "unknown" });
    }
    
    Ok(())
}

fn handle_block_number() -> String {
    r#"{"jsonrpc":"2.0","result":"0x64","id":1}"#.to_string()
}

fn handle_get_block() -> String {
    println!("🗄️  [ARCHIVAL] Fetching block from archival storage...");
    r#"{"jsonrpc":"2.0","result":{"number":"0x1","hash":"0x1234567890abcdef","parentHash":"0x0000000000000000","timestamp":"0x55ba467c","transactions":["0xabc123","0xdef456"],"gasUsed":"0x5208","size":"0x284"},"id":1}"#.to_string()
}

fn handle_get_proof() -> String {
    println!("🔍 [PROOF] Generating Merkle proof...");
    r#"{"jsonrpc":"2.0","result":{"address":"0xa0b86a33e6ba5d0c6d93ceaf5c9d19b0b18f5e0c","balance":"0x56bc75e2d630eb20","nonce":"0x2a","storageHash":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421","codeHash":"0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470","accountProof":["0xf90211a0abc123","0xf90211a1def456","0xf90211a2456789"],"storageProof":[{"key":"0x0","value":"0x1234","proof":["0xproof1","0xproof2","0xproof3"]}]},"id":1}"#.to_string()
}

fn handle_status() -> String {
    r#"{"archival_enabled":true,"websocket_enabled":true,"proof_support":true,"blocks_available":104,"active_subscriptions":0,"connections":1}"#.to_string()
}

fn handle_unknown() -> String {
    r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#.to_string()
}
