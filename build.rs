// Embeds Windows version-info + icon into the exe so it shows a proper name and
// icon in Explorer, the taskbar, and the file's Properties → Details.
fn main() {
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
