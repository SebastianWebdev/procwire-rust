//! Stream Worker - example of streaming data to parent.
//!
//! This example demonstrates:
//! - Creating a streaming handler with `handle_stream`
//! - Sending multiple chunks with `ctx.chunk()`
//! - Ending the stream with `ctx.end()`
//!
//! # Running with Node.js parent
//!
//! ```js
//! import { Module } from '@procwire/core';
//!
//! const mod = new Module('generator')
//!     .executable('./target/debug/examples/stream')
//!     .method('generate', { response: 'stream' });
//!
//! await mod.spawn();
//!
//! for await (const chunk of mod.stream('generate', { count: 5 })) {
//!     console.log(chunk); // { index: 0 }, { index: 1 }, ...
//! }
//!
//! await mod.kill();
//! ```

use procwire_client::{Client, RequestContext};
use serde::{Deserialize, Serialize};

/// Input structure for the generate method.
#[derive(Deserialize, Debug)]
struct GenerateInput {
    count: usize,
}

/// Chunk structure sent in the stream.
#[derive(Serialize, Debug)]
struct Chunk {
    index: usize,
    data: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        // Register a streaming handler
        .handle_stream(
            "generate",
            |data: GenerateInput, ctx: RequestContext| async move {
                // Send a series of chunks
                for i in 0..data.count {
                    ctx.chunk(&Chunk {
                        index: i,
                        data: format!("Chunk {}", i),
                    })
                    .await?;

                    // Simulate some work
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }

                // End the stream (sends STREAM_END frame with empty payload)
                ctx.end().await
            },
        )
        .start()
        .await?;

    client.wait_for_shutdown().await?;

    Ok(())
}
