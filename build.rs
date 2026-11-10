fn main() {
    // Embed the icon and version info into the .exe (Explorer, taskbar, UAC prompt).
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/envoy.ico");
        res.set("ProductName", "Envoy");
        res.set("FileDescription", "Envoy - Windows environment variable manager");
        if let Err(e) = res.compile() {
            // Don't fail the whole build over a cosmetic resource.
            println!("cargo:warning=could not embed icon: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/envoy.ico");
    println!("cargo:rerun-if-changed=build.rs");
}
