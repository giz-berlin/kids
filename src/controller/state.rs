use crate::{source, target};

#[derive(Debug)]
pub struct AppState<S: source::interface::Source + Send, T: target::interface::Target> {
    pub source: std::sync::Arc<S>,
    pub target: std::sync::Arc<tokio::sync::RwLock<T>>,
}

impl<S: source::interface::Source + Send, T: target::interface::Target> Clone for AppState<S, T> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            target: self.target.clone(),
        }
    }
}
