fn main() {
    // Embed the application icon only in Windows builds (via windres).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("assets/easyshare.rc", embed_resource::NONE);
    }
}
