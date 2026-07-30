//! 数据仓库层 trait 定义
//! 封装对 core-storage 的访问，业务层只依赖此 trait，不直接操作 SQL

/// 数据仓库标记 trait，当前为空骨架，后续会扩展 CRUD 方法
pub trait Repository: Send + Sync {}
