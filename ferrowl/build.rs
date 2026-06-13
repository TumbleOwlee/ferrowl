fn main() {
    // build.rs runs on the host (Linux in CI), so gate on the *target* OS rather
    // than cfg!(windows). winresource derives the `<triple>-windres` tool name from
    // TARGET, so the mingw-w64 cross toolchain is picked up for x86_64-pc-windows-gnu.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../images/ferrowl.ico");
        res.compile().expect("failed to embed Windows resource");
    }
}
