fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");

    #[cfg(windows)]
    {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/app-icon.ico");
        resource.set("FileDescription", "Browser Harbor");
        resource.set("ProductName", "Browser Harbor");
        resource.set("CompanyName", "MagicCube99");
        resource.set("OriginalFilename", "browser-harbor.exe");
        let comments = format!("Browser Harbor | Built by MagicCube99 | Browser Harbor v{version}");
        let private_build = format!("MagicCube99/browser-harbor/{version}");
        resource.set("Comments", &comments);
        resource.set("PrivateBuild", &private_build);
        if let Err(error) = resource.compile() {
            panic!("failed to embed Windows application icon: {error}");
        }
    }
}
