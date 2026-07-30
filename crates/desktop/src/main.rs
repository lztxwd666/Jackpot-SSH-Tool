//! 桌面应用二进制入口
//! windows_subsystem 属性确保 release 模式下不弹出控制台窗口

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    desktop::run();
}
