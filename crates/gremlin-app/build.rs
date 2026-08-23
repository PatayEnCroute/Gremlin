//! Build script pour intégrer l'icône officielle de l'application dans les métadonnées Windows.

fn main() {
    #[cfg(target_os = "windows")]
    {
        let icon_path = std::path::Path::new("../../assets/icon.ico");
        if icon_path.exists() {
            if let Some(icon_str) = icon_path.to_str() {
                let mut res = winres::WindowsResource::new();
                res.set_icon(icon_str);
                let _ = res.compile();
            }
        }
    }
}
