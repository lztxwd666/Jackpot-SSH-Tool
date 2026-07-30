use std::path::Path;

pub trait Config: Send + Sync {
    fn app_data_dir(&self) -> &Path;
    fn log_level(&self) -> &str;
}

pub struct DefaultConfig {
    data_dir: std::path::PathBuf,
    log_level: String,
}

impl DefaultConfig {
    pub fn new(data_dir: std::path::PathBuf, log_level: String) -> Self {
        Self { data_dir, log_level }
    }
}

impl Config for DefaultConfig {
    fn app_data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn log_level(&self) -> &str {
        &self.log_level
    }
}
