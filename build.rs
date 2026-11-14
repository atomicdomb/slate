//! On macOS, compile the small Objective-C file that receives "open this
//! file" requests from Finder. Nothing to do on other systems.

fn main() {
    println!("cargo:rerun-if-changed=src/macos_open.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new().file("src/macos_open.m").flag("-fobjc-arc").compile("slate_macos_open");
        println!("cargo:rustc-link-lib=framework=AppKit");
    }
}
