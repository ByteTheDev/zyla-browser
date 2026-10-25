// Embeds the Z logo as the executable's icon, so Explorer and the taskbar show the project logo.
fn main() {
    println!("cargo:rerun-if-changed=../assets/logo.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("../assets/logo.ico");
        resource.set("FileDescription", "Zyla Browser");
        resource.set("ProductName", "Zyla Browser");
        resource.compile().expect("could not embed the application icon");
    }
}
