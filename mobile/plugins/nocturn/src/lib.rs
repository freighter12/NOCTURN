//! Native iPhone parts of Nocturn Music. All commands live in Swift; this
//! crate only registers the Swift plugin so Tauri can route calls to it.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_nocturn);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("nocturn")
        .setup(|_app, _api| {
            #[cfg(target_os = "ios")]
            _api.register_ios_plugin(init_plugin_nocturn)?;
            Ok(())
        })
        .build()
}
