//! On Windows, put WoodSketch's icon in the program file itself, so the Start menu, taskbar
//! and Explorer show it (the window sets its own icon at run time).

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/woodsketch.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/icon/woodsketch.ico");
        resource.compile().expect("embed the Windows icon");
    }
}
