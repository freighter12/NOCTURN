// Every command here is implemented in Swift (ios/Sources). Tauri routes
// `plugin:nocturn|<command>` calls that Rust doesn't handle to the Swift
// method with the camelCase name (set_queue → setQueue).
const COMMANDS: &[&str] = &[
    "set_queue",
    "insert",
    "play",
    "pause",
    "toggle",
    "next",
    "previous",
    "seek",
    "set_repeat",
    "set_autoplay",
    "jump",
    "get_state",
    "set_tabs",
    "select_tab",
    "set_lyrics",
    "show_now_playing",
    "haptic",
    "replace_upcoming",
    "set_ai_shuffle",
    "download",
    "remove_downloads",
    "get_downloads",
    "get_settings",
    "set_theme",
    "set_live_lyrics",
    "set_look",
    "register_listener",
    "remove_listener",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
