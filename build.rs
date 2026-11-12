fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        // Icon id 1 is what Explorer shows and what the window loads at runtime.
        res.set_icon_with_id("assets/app.ico", "1");
        res.set_manifest_file("assets/app.manifest");
        // "FileDescription" is the name Windows shows in "Open with" and Task Manager.
        res.set("FileDescription", "Markdown");
        res.set("ProductName", "Markdown");
        res.set("OriginalFilename", "Markdown.exe");
        res.set("InternalName", "Markdown");
        res.set("CompanyName", "Markdown");
        res.set("LegalCopyright", "MIT License");
        res.compile().expect("failed to compile Windows resources");
    }
}
