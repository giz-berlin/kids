#[derive(Debug)]
pub struct AppState<S: crate::interface::source::Source + Send, T: crate::interface::target::Target> {
    pub source: std::sync::Arc<S>,
    pub target: std::sync::Arc<tokio::sync::RwLock<T>>,
}

impl<S: crate::interface::source::Source + Send, T: crate::interface::target::Target> Clone for AppState<S, T> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            target: self.target.clone(),
        }
    }
}
