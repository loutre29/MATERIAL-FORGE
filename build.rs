// Embeds the Material Forge icon in the Windows executable.
fn main() {
    println!("cargo:rerun-if-changed=assets/material_forge.ico");
    #[cfg(windows)]
    {
        if std::path::Path::new("assets/material_forge.ico").exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/material_forge.ico");
            res.set("ProductName", "Material Forge");
            res.set("FileDescription", "Material Forge");
            if let Err(e) = res.compile() {
                println!("cargo:warning=icon not embedded: {e}");
            }
        }
    }
}
