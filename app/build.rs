#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=resources/app_icon.ico");
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("failed to compile Windows resource file app.rc");
}

#[cfg(not(windows))]
fn main() {}
