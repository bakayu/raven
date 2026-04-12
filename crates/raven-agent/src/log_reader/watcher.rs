use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum TailerEvent {
    Modified(PathBuf),
    Rotated(PathBuf),
    Created(PathBuf),
}

pub struct Watcher {}

impl Watcher {
    pub fn new() -> anyhow::Result<Self> {
        todo!()
    }

    pub fn add_file(&mut self, path: &Path) -> anyhow::Result<()> {
        todo!()
    }

    pub fn add_parent_dir(&mut self, path: &Path) -> anyhow::Result<()> {
        todo!()
    }

    pub fn next_events(&mut self) -> anyhow::Result<Vec<TailerEvent>> {
        todo!()
    }
}
