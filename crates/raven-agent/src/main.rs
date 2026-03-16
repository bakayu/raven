use std::error::Error;
use tokio::sync::mpsc;

use raven_agent::{collector_task, heartbeat_task, transport_task};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (tx, rx) = mpsc::channel(256);

    let heartbeat_task_handle = tokio::spawn(heartbeat_task(tx.clone()));
    let collector_task_handle = tokio::spawn(collector_task(tx.clone()));
    let transport_task_handle = tokio::spawn(transport_task(rx));

    tokio::try_join!(
        heartbeat_task_handle,
        collector_task_handle,
        transport_task_handle
    )?;

    Ok(())
}
