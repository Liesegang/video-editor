// ... (keep imports)
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PluginConfig {
    pub paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ThemeType {
    Dark,
    Light,
    Latte,
    Frappe,
    Macchiato,
    Mocha,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ThemeConfig {
    pub theme_type: ThemeType,
}

#[derive(Serialize, Deserialize)]
struct ShortcutDefWrapper {
    #[serde(with = "ModifiersDef")]
    modifiers: Modifiers,
    #[serde(with = "KeyDef")]
    key: Key,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AppConfig {
    #[serde(with = "tuple_vec_map")]
    pub shortcuts: HashMap<CommandId, Option<(Modifiers, Key)>>,
    pub plugins: PluginConfig,
    pub theme: ThemeConfig,
}

impl AppConfig {
    pub fn new() -> Self {
        Self {
            shortcuts: HashMap::new(),
            plugins: PluginConfig {
                paths: vec!["./assets/plugins/sksl".to_string()],
            },
            theme: ThemeConfig {
                theme_type: ThemeType::Dark,
            },
        }
    }
}

mod tuple_vec_map {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct SerializableTuple(CommandId, Option<ShortcutDefWrapper>);

    pub fn serialize<S>(
        map: &HashMap<CommandId, Option<(Modifiers, Key)>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let vec: Vec<_> = map
            .iter()
            .map(|(id, opt_shortcut)| {
                let wrapped = opt_shortcut.map(|(m, k)| ShortcutDefWrapper {
                    modifiers: m,
                    key: k,
                });
                SerializableTuple(*id, wrapped)
            })
            .collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<CommandId, Option<(Modifiers, Key)>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<SerializableTuple> = Vec::deserialize(deserializer)?;
        Ok(vec
            .into_iter()
            .map(|SerializableTuple(id, wrapped)| {
                let opt = wrapped.map(|w| (w.modifiers, w.key));
                (id, opt)
            })
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
        let config_path = config_dir.join("config.toml");
        return Some(config_path);
    }
    None
}

pub fn save_config(config: &AppConfig) {
    if let Some(path) = get_config_path() {
        match toml::to_string_pretty(config) {
            Ok(toml_str) => {
                if let Err(e) = fs::write(&path, toml_str) {
                    error!("Failed to write config file: {}", e);
                } else {
                    info!("Config saved to {}", path.display());
                }
            }
            Err(e) => {
                error!("Failed to serialize config: {}", e);
            }
        }
    }
}

pub fn load_config() -> AppConfig {
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
    let default_config = AppConfig::new();
    // Verify if we should save. If path exists but failed to load, we might not want to overwrite?
    // The user request "if config file didn't exist".
    if let Some(path) = get_config_path() {
        if !path.exists() {
            save_config(&default_config);
        }
    }
    default_config
}
