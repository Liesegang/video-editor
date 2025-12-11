use crate::command::CommandId;
use directories::ProjectDirs;
use eframe::egui::{Key, Modifiers};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use toml;

// --- Serde remote definitions for egui types ---

#[derive(Serialize, Deserialize)]
#[serde(remote = "Modifiers")]
struct ModifiersDef {
    alt: bool,
    ctrl: bool,
    shift: bool,
    mac_cmd: bool,
    command: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Key")]
enum KeyDef {
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Escape,
    Tab,
    Backspace,
    Delete,
    Enter,
    Space,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Copy,
    Cut,
    Paste,

    // Symbols
    Colon,
    Comma,
    Backslash,
    Slash,
    Pipe,
    Questionmark,
    Plus,
    Minus,
    Quote,
    OpenBracket,
    CloseBracket,
    Semicolon,
    Period,
    Equals,
    Backtick,

    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    F25,
    F26,
    F27,
    F28,
    F29,
    F30,
    F31,
    F32,
    F33,
    F34,
    F35,
    Exclamationmark,
    OpenCurlyBracket,
    CloseCurlyBracket,
    BrowserBack,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ShortcutConfig {
    #[serde(with = "tuple_vec_map")]
    pub shortcuts: HashMap<CommandId, (Modifiers, Key)>,
}

impl ShortcutConfig {
    pub fn new() -> Self {
        Self {
            shortcuts: HashMap::new(),
        }
    }
}

mod tuple_vec_map {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct SerializableTuple(
        CommandId,
        #[serde(with = "ModifiersDef")] Modifiers,
        #[serde(with = "KeyDef")] Key,
    );

    pub fn serialize<S>(
        map: &HashMap<CommandId, (Modifiers, Key)>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let vec: Vec<_> = map
            .iter()
            .map(|(id, (m, k))| SerializableTuple(*id, *m, *k))
            .collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<CommandId, (Modifiers, Key)>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<SerializableTuple> = Vec::deserialize(deserializer)?;
        Ok(vec
            .into_iter()
            .map(|SerializableTuple(id, m, k)| (id, (m, k)))
            .collect())
    }
}

fn get_config_path() -> Option<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from("me", "liesegang", "video_editor") {
        let config_dir = proj_dirs.config_dir();
        if !config_dir.exists() {
            if let Err(e) = fs::create_dir_all(config_dir) {
                error!("Failed to create config directory: {}", e);
                return None;
            }
        }
        let config_path = config_dir.join("shortcuts.toml");
        return Some(config_path);
    }
    None
}

pub fn save_config(config: &ShortcutConfig) {
    if let Some(path) = get_config_path() {
        match toml::to_string_pretty(config) {
            Ok(toml_str) => {
                if let Err(e) = fs::write(&path, toml_str) {
                    error!("Failed to write config file: {}", e);
                } else {
                    info!("Shortcuts saved to {}", path.display());
                }
            }
            Err(e) => {
                error!("Failed to serialize config: {}", e);
            }
        }
    }
}

pub fn load_config() -> ShortcutConfig {
    if let Some(path) = get_config_path() {
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(toml_str) => match toml::from_str(&toml_str) {
                    Ok(config) => return config,
                    Err(e) => {
                        warn!("Failed to parse config file, using defaults: {}", e);
                    }
                },
                Err(e) => {
                    warn!("Failed to read config file, using defaults: {}", e);
                }
            }
        }
    }
    // Return default if file doesn't exist or on any error
    ShortcutConfig::new()
}
