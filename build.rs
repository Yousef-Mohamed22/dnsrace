//! Windows executable metadata.
//!
//! The icon and version resource only exist on Windows; every other platform compiles this
//! build script to an empty `main`.

fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("packaging/icon.ico");
        resource.set("ProductName", "dnsrace");
        resource.set(
            "FileDescription",
            "Find the fastest DNS resolver for your connection",
        );
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        resource.set("ProductVersion", &version);
        resource.set("FileVersion", &version);
        if let Err(error) = resource.compile() {
            // A missing resource compiler must not make the Windows build unusable.
            println!("cargo:warning=could not embed Windows resources: {error}");
        }
    }
}
