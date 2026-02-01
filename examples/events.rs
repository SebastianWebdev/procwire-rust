//! Event Worker - example of emitting events to parent.
//!
//! This example demonstrates:
//! - Registering events with `.event()`
//! - Using ACK response type with `handle_ack`
//! - Emitting events to parent with `client.emit()`
//!
//! # Running with Node.js parent
//!
//! ```js
//! import { Module } from '@procwire/core';
//!
//! const mod = new Module('progress')
//!     .executable('./target/debug/examples/events')
//!     .method('start_work', { response: 'ack' })
//!     .event('progress');
//!
//! await mod.spawn();
//!
//! mod.onEvent('progress', (data) => {
//!     console.log(`Progress: ${data.percent}%`);
//! });
//!
//! await mod.send('start_work', { steps: 10 });
//! await mod.kill();
//! ```

use procwire_client::{Client, RequestContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Input structure for the start_work method.
#[derive(Deserialize, Debug)]
struct WorkInput {
    steps: u32,
}

/// Progress event structure.
#[derive(Serialize, Debug)]
struct ProgressEvent {
    percent: u32,
    message: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // We'll need to share the client for emitting events
    // Use a channel to communicate between handler and main loop
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(u32, u32)>(10);

    let client = Client::builder()
        // Register an ACK handler
        .handle_ack("start_work", move |data: WorkInput, ctx: RequestContext| {
            let tx = tx.clone();
            async move {
                // Send ACK immediately
                ctx.ack().await?;

                // Send work parameters to the event emitter
                tx.send((data.steps, 0)).await.ok();

                Ok(())
            }
        })
        // Register the progress event
        .event("progress")
        .start()
        .await?;

    // Store client in Arc for sharing
    let client = Arc::new(Mutex::new(Some(client)));
    let client_for_events = client.clone();

    // Spawn event emitter task
    tokio::spawn(async move {
        while let Some((steps, _)) = rx.recv().await {
            // Simulate work and emit progress events
            for i in 1..=steps {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                let percent = (i * 100) / steps;
                let event = ProgressEvent {
                    percent,
                    message: format!("Step {} of {}", i, steps),
                };

                // Emit progress event
                if let Some(ref c) = *client_for_events.lock().await {
                    if let Err(e) = c.emit("progress", &event).await {
                        eprintln!("Failed to emit event: {}", e);
                        break;
                    }
                }
            }
        }
    });

    // Wait for shutdown
    let c = client.lock().await.take();
    if let Some(c) = c {
        c.wait_for_shutdown().await?;
    }

    Ok(())
}
