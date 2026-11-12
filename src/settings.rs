//! Per-user settings (%LOCALAPPDATA%\Markdown\settings.json), UI language and system theme.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub wrap: bool,
    pub zoom: f64,
    pub split: f64,
    pub width: f64,
    pub height: f64,
    pub maximized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: "auto".into(), wrap: true, zoom: 1.0, split: 0.5, width: 1040.0, height: 780.0, maximized: false }
    }
}

impl Settings {
    pub fn ui_json(&self) -> Value {
        json!({ "theme": self.theme, "wrap": self.wrap, "zoom": self.zoom, "split": self.split })
    }

    pub fn merge_ui(&mut self, v: &Value) {
        if let Some(t) = v["theme"].as_str()
            && matches!(t, "auto" | "light" | "dark")
        {
            self.theme = t.to_string();
        }
        if let Some(w) = v["wrap"].as_bool() {
            self.wrap = w;
        }
        if let Some(z) = v["zoom"].as_f64() {
            self.zoom = z.clamp(0.5, 3.0);
        }
        if let Some(s) = v["split"].as_f64() {
            self.split = s.clamp(0.15, 0.85);
        }
    }

    pub fn wants_dark(&self) -> bool {
        match self.theme.as_str() {
            "dark" => true,
            "light" => false,
            _ => system_dark(),
        }
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Markdown")
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read(settings_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(s: &Settings) {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(s) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// "ru" for Russian Windows, "en" otherwise.
pub fn lang() -> &'static str {
    #[cfg(windows)]
    {
        let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        if id & 0x3ff == 0x19 {
            return "ru";
        }
    }
    "en"
}

pub fn system_dark() -> bool {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        if let Ok(k) = key
            && let Ok(v) = k.get_value::<u32, _>("AppsUseLightTheme")
        {
            return v == 0;
        }
    }
    false
}

/// Strings used by native dialogs.
pub fn tr(lang: &str, key: &str) -> &'static str {
    let ru = lang == "ru";
    match key {
        "untitled" => if ru { "Без имени" } else { "Untitled" },
        "save_changes" => if ru { "Сохранить изменения в «{}»?" } else { "Do you want to save changes to \"{}\"?" },
        "save_failed" => if ru { "Не удалось сохранить файл" } else { "Could not save the file" },
        "open_failed" => if ru { "Не удалось открыть файл" } else { "Could not open the file" },
        "too_large" => if ru { "Файл слишком большой для открытия (больше 64 МБ)." } else { "The file is too large to open (over 64 MB)." },
        "not_found" => if ru { "Файл не найден: {}" } else { "File not found: {}" },
        "saved_utf8" => if ru { "Сохранено в UTF-8: часть символов нельзя записать в {}" } else { "Saved as UTF-8: some characters can't be stored in {}" },
        "filter_md" => "Markdown",
        "filter_text" => if ru { "Текстовые файлы" } else { "Text files" },
        "filter_all" => if ru { "Все файлы" } else { "All files" },
        "open_title" => if ru { "Открыть" } else { "Open" },
        "save_title" => if ru { "Сохранить как" } else { "Save as" },
        "doc_type" => if ru { "Документ Markdown" } else { "Markdown document" },
        "text_type" => if ru { "Текстовый документ" } else { "Text document" },
        "verb" => if ru { "Открыть в Markdown" } else { "Open with Markdown" },
        "app_desc" => if ru {
            "Лёгкий просмотрщик и редактор Markdown с оформлением как на GitHub"
        } else {
            "Lightweight Markdown viewer and editor with GitHub-style preview"
        },
        "installed" => if ru {
            "Markdown зарегистрирован в Windows.\n\n\
             • Win+R → markdown\n\
             • Меню «Пуск» → Markdown\n\
             • «Открыть с помощью» → Markdown для .md, .txt и других текстовых файлов\n\
             • Правый клик по любому файлу → «Открыть в Markdown»\n\n\
             Файлы .md, у которых ещё не было программы по умолчанию, теперь открываются в Markdown. \
             Чтобы назначить его для .md или .txt вместо другой программы: \
             Параметры → Приложения → Приложения по умолчанию → Markdown."
        } else {
            "Markdown is now registered in Windows.\n\n\
             • Win+R → markdown\n\
             • Start menu → Markdown\n\
             • \"Open with\" → Markdown for .md, .txt and other text files\n\
             • Right-click any file → \"Open with Markdown\"\n\n\
             .md files without a default app now open in Markdown. \
             To make it the default for .md or .txt instead of another app: \
             Settings → Apps → Default apps → Markdown."
        },
        "install_failed" => if ru { "Не удалось зарегистрировать Markdown" } else { "Could not register Markdown" },
        "in_use" => if ru {
            "Установленная копия Markdown сейчас запущена. Закройте все окна Markdown и повторите."
        } else {
            "The installed copy of Markdown is running. Close all Markdown windows and try again."
        },
        "uninstall_confirm" => if ru {
            "Удалить Markdown из Windows?\n\nБудут удалены ассоциации файлов, ярлык в меню «Пуск», команда Win+R и настройки. Ваши документы не затрагиваются."
        } else {
            "Remove Markdown from Windows?\n\nFile associations, the Start menu shortcut, the Win+R command and settings will be removed. Your documents are not touched."
        },
        "uninstalled" => if ru { "Markdown удалён из Windows." } else { "Markdown has been removed from Windows." },
        _ => "",
    }
}
