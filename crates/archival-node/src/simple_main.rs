use reth_archival_node::simple::SimpleArchivalNode;
use reth_chainspec::MAINNET;
use eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting Reth Archival Node");
    
    // Create and start simple archival node
    let node = SimpleArchivalNode::new(MAINNET.clone());
    node.start().await?;
    
    Ok(())
}
