//! 应用配置抽象模块

use std::path::Path;

/// 应用程序运行时的核心配置抽象
/// 各层通过 trait object 访问配置，无需关心具体实现
pub trait Config: Send + Sync {
    fn app_data_dir(&self) -> &Path;
    fn log_level(&self) -> &str;
}

/// Config trait 的默认实现（纯注入：路径与日志级别由调用方提供，本类型不读取任何配置）
pub struct DefaultConfig {
    data_dir: std::path::PathBuf,
    log_level: String,
}

impl DefaultConfig {
    pub fn new(data_dir: std::path::PathBuf, log_level: String) -> Self {
        Self {
            data_dir,
            log_level,
        }
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
