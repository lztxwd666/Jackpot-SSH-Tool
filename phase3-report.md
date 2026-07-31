# Phase 3: Host Management - Implementation Report

## 状态

**通过** - build、test、clippy 全部通过，无警告。

## 实现内容

### 1. Host 模型 + HostRepository trait (`core-common`)
- `crates/core-common/src/host.rs` - `Host` 结构体（11 个字段）和 `HostRepository` trait（5 个方法）
- `crates/core-common/src/lib.rs` - 添加 `pub mod host` 和重导出

### 2. HostEvent (`core-event`)
- `crates/core-event/src/event.rs` - `HostEvent` 枚举（Created/Updated/Deleted），添加到 `CoreEvent::Host(HostEvent)`
- `crates/core-event/src/lib.rs` - 重导出 `HostEvent`

### 3. SqliteHostRepository (`core-storage`)
- `crates/core-storage/src/host_repo.rs` - `HostRepository` trait 的 SQLite 实现
  - `list_all()` / `find_by_id()` / `save()` / `delete()` / `search()`
  - INTEGER ↔ bool 映射 (0/1)
  - 自动生成 UTC ISO 8601 时间戳
  - 4 个单元测试
- `crates/core-storage/src/lib.rs` - 添加模块和重导出
- `crates/core-storage/Cargo.toml` - 添加 `uuid` 依赖

### 4. CoreRuntime 集成 (`core-runtime`)
- `crates/core-runtime/src/runtime.rs` - 添加 `host_repo` 字段、在 `start()` 中创建、添加 `host_repo()` 访问器、在 `shutdown()` 中清理
- `crates/core-runtime/src/lib.rs` - 重导出 `HostRepository`

### 5. IPC 命令 (`desktop`)
- `crates/desktop/src/commands.rs` - 4 个命令：`list_hosts`、`save_host`、`delete_host`、`search_hosts`
- `crates/desktop/src/lib.rs` - 注册所有新命令到 `invoke_handler`
- `crates/desktop/Cargo.toml` - 添加 `uuid` 依赖

### 6. Vue 前端 (`desktop/ui`)
- `src/App.vue` - 完整重写，包含：
  - 左侧边栏：主机列表 + 搜索栏 + 添加按钮
  - 右侧面板：主机表单（名称/地址/端口/用户名/认证类型/分组/备注/收藏）
  - 保存/删除/取消操作
  - 监听 `core-event` 事件自动刷新列表
  - 使用 `@tauri-apps/api/core` 中的 `invoke` 函数
- `src/assets/main.css` - 移除 conflicting grid layout

## 测试结果

```
running 8 tests
test db::tests::test_open_and_migrate ... ok
test db::tests::test_hosts_table_exists ... ok
test host_repo::tests::test_save_and_list ... ok
test host_repo::tests::test_save_and_find_by_id ... ok
test host_repo::tests::test_delete ... ok
test host_repo::tests::test_search ... ok
test knownhosts::tests::test_store_and_find_host_key ... ok
test knownhosts::tests::test_remove_host_key ... ok
test result: ok. 8 passed; 0 failed
```

（另有 3 个 core-event 测试和 1 个 core-runtime 测试通过，总共 12 个测试）

## 文件变更

| 文件 | 操作 |
| --- | --- |
| `crates/core-common/src/host.rs` | 新建 |
| `crates/core-common/src/lib.rs` | 修改 |
| `crates/core-event/src/event.rs` | 修改 |
| `crates/core-event/src/lib.rs` | 修改 |
| `crates/core-storage/src/host_repo.rs` | 新建 |
| `crates/core-storage/src/lib.rs` | 修改 |
| `crates/core-storage/Cargo.toml` | 修改 |
| `crates/core-runtime/src/runtime.rs` | 修改 |
| `crates/core-runtime/src/lib.rs` | 修改 |
| `crates/desktop/src/commands.rs` | 修改 |
| `crates/desktop/src/lib.rs` | 修改 |
| `crates/desktop/Cargo.toml` | 修改 |
| `crates/desktop/ui/src/App.vue` | 重写 |
| `crates/desktop/ui/src/assets/main.css` | 修改 |
