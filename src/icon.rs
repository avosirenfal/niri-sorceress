use freedesktop_icons::lookup;
use std::fs;
use std::path::{Path, PathBuf};

/// Primary function to get an icon's path based on an app_id.
pub fn get_app_icon_path(app_id: &str, size: u16) -> Option<PathBuf> {
    // 1. Find the corresponding .desktop file
    let desktop_path = find_desktop_file_by_name(app_id)
        .or_else(|| search_desktop_files_for_wm_class(app_id))?;

    // 2. Read the .desktop file and extract the "Icon=" value
    let icon_name = get_icon_name_from_desktop(&desktop_path)?;

    // 3. Resolve the icon name to a file path
    resolve_icon_path(&icon_name, size)
}

/// Tries to find the .desktop file directly by file name (e.g., in /usr/share/applications/)
fn find_desktop_file_by_name(app_id: &str) -> Option<PathBuf> {
    let xdg_dirs = xdg::BaseDirectories::new();

    // Equivalent to your prefix + folder + suffix logic
    let search_names = [
        format!("{}.desktop", app_id),
        app_id.to_string(),
    ];

    let sub_folders = ["applications", "applications/kde", "applications/org.kde"];

    for folder in &sub_folders {
        for file_name in &search_names {
            let relative_path = Path::new(folder).join(file_name);
            // find_data_file searches XDG_DATA_HOME and XDG_DATA_DIRS
            if let Some(path) = xdg_dirs.find_data_file(&relative_path) {
                return Some(path);
            }
        }
    }

    None
}

/// Fallback: Equivalent to `g_desktop_app_info_search`. Scans desktop files
/// to see if `StartupWMClass` matches the app_id.
fn search_desktop_files_for_wm_class(app_id: &str) -> Option<PathBuf> {
    let xdg_dirs = xdg::BaseDirectories::new();

    // Combine local user data dir and system data dirs
    let mut dirs = xdg_dirs.get_data_dirs();
    if let Some(home) = xdg_dirs.get_data_home() {
        dirs.insert(0, home);
    }

    let target_line = format!("StartupWMClass={}", app_id);

    for dir in dirs {
        let apps_dir = dir.join("applications");
        if !apps_dir.exists() {
            continue;
        }

        // Iterate through the directory
        if let Ok(entries) = fs::read_dir(apps_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        // If we find the matching StartupWMClass, return this path
                        if content.lines().any(|line| line.trim() == target_line) {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    None
}

/// Very simple INI parser to grab the "Icon=" key from a .desktop file.
fn get_icon_name_from_desktop(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("Icon=") {
            // Return everything after "Icon="
            return Some(line[5..].to_string());
        }
    }

    None
}

/// Takes the raw icon string (either a name like "firefox" or an absolute path)
/// and resolves it to an actual file on disk.
fn resolve_icon_path(icon_value: &str, size: u16) -> Option<PathBuf> {
    // Sometimes .desktop files hardcode absolute paths to icons
    let direct_path = Path::new(icon_value);
    if direct_path.is_absolute() && direct_path.exists() {
        return Some(direct_path.to_path_buf());
    }

    // Otherwise, it's a theme icon name. Use `freedesktop-icons` to look it up.
    // This crate handles checking /usr/share/icons, ~/.local/share/icons, hicolor fallbacks, etc.
    lookup(icon_value)
        .with_size(size)
        // Optionally add `.with_theme("Yaru")` if you want to support specific themes,
        // otherwise it defaults to the standard fallback ("hicolor")
        .find()
}