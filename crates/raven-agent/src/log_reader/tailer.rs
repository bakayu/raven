use tokio::sync::mpsc;

use super::model::LogBatch;
use crate::configuration::LogSource;

pub struct LogTailer {}

impl LogTailer {
    pub fn new(sources: Vec<LogSource>, tx: mpsc::Sender<LogBatch>) -> anyhow::Result<Self> {
        todo!()
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        todo!()
    }
}
