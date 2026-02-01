//! Echo Worker - simple request/response example.
//!
//! This example demonstrates:
//! - Creating a Procwire client with the builder pattern
//! - Registering a method handler with typed input/output
//! - Sending a response back to the parent
//!
//! # Running with Node.js parent
//!
//! ```js
//! import { Module } from '@procwire/core';
//!
//! const mod = new Module('echo')
//!     .executable('./target/debug/examples/echo')
//!     .method('echo', { response: 'result' });
//!
//! await mod.spawn();
//! const result = await mod.send('echo', { message: 'hello' });
//! console.log(result); // { echo: 'hello' }
//! await mod.kill();
//! ```

use procwire_client::{Client, RequestContext};
use serde::{Deserialize, Serialize};

/// Input structure for the echo method.
#[derive(Deserialize, Debug)]
struct EchoInput {
    message: String,
}

/// Output structure for the echo method.
#[derive(Serialize, Debug)]
struct EchoOutput {
    echo: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build the client with fluent API
    let client = Client::builder()
        // Register the "echo" method handler
        .handle("echo", |data: EchoInput, ctx: RequestContext| async move {
            // Respond with the same message
            ctx.respond(&EchoOutput { echo: data.message }).await
        })
        .start()
        .await?;

    // Wait for shutdown (parent closes the pipe)
    client.wait_for_shutdown().await?;

    Ok(())
}
