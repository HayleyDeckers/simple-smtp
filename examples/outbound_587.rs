use anyhow::Result;
use simple_smtp::{Smtp, integrations::tokio::TokioIo};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to codingcat.nl on submission port 587
    let server = "codingcat.nl:587";
    let tcp_stream = TcpStream::connect(server).await?;

    // Wrap the TcpStream with TokioIo
    let tcp_stream = TokioIo(tcp_stream);

    // Create an SMTP client
    let mut smtp = Smtp::new(tcp_stream);

    // Wait for the server to be ready
    smtp.ready().await?;

    // Send EHLO command
    let ehlo_response = smtp.ehlo("example.com").await?;
    // Print the EHLO response extensions

    for line in ehlo_response.lines() {
        println!("{line}");
    }
    // Disconnect
    smtp.quit().await?;
    println!("Disconnected from server.");

    Ok(())
}
