// No console window behind the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK's DMA-BUF renderer draws blank windows on many Intel/Mesa setups
    // (Ubuntu 22.04 among them). Must be set before GTK starts.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    kumquat_lib::run()
}
