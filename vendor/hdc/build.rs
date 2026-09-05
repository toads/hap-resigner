use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(".");
    let cffi = root.join("src/cffi");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS");
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let mut build = cc::Build::new();
    build.cpp(true).std("c++20").define("HDC_HOST", None);
    if target_os == "macos" {
        build.define("HOST_MAC", None);
    }
    build.include(&cffi).include(root.join("compat")).include(root.join("compat/include"));
    if target_os == "windows" && target_env == "msvc" {
        build.static_crt(true);
    }
    let mut sources = vec![
        "host/ctimer.cpp",
        "host/host_usb.cpp",
        "host/host_usb_wrapper.cpp",
        "serial_struct.cpp",
        "transfer.cpp",
        "utils.cpp",
    ];
    if target_os != "windows" {
        sources.push("usb_util.cpp");
        sources.push("sendmsg.cpp");
    }
    for source in sources {
        build.file(cffi.join(source));
    }
    build.file(root.join("compat/usb_device_shim.cpp"));
    build.compile("hdc_cffi_host");

    println!("cargo:rerun-if-changed={}", cffi.display());
    println!("cargo:rerun-if-changed=compat/securec.h");
}

