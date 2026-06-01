// Embeds Windows version-info + icon into the exe so it shows a proper name and
// icon in Explorer, the taskbar, and the file's Properties → Details, and
// compiles the Slint settings UI.
fn main() {
    // Embed referenced resources (the bundled .ttf fonts) into the binary so it
    // stays self-contained and portable rather than referencing build paths.
    let cfg = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/app.slint", cfg).expect("compiling Slint UI");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/sundial.ico");
        res.set("FileDescription", "HDR Sundial");
        res.set("ProductName", "HDR Sundial");
        res.set(
            "Comments",
            "Sets Windows SDR content brightness from the sun",
        );
        res.set("CompanyName", "evaera");
        res.set("LegalCopyright", "Copyright (c) 2026 evaera");
        res.compile().expect("embedding Windows resources");
    }
}
