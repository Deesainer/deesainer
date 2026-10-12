fn main() {
    #[cfg(windows)]
    {
        use std::path::PathBuf;
        let icon =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/app-icon/deesainer.ico");
        if let Some(icon) = icon.to_str() {
            if let Err(err) = winres::WindowsResource::new().set_icon(icon).compile() {
                println!("cargo:warning=icon resource not embedded: {err}");
            }
        }
    }
}
