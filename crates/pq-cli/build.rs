fn main() {
    let version = match std::env::var("BUILD_VERSION") {
        Ok(v) if !v.is_empty() => v,
        _ => format!("{} dev", env!("CARGO_PKG_VERSION")),
    };
    println!("cargo:rustc-env=PQ_VERSION={version}");
    println!("cargo:rerun-if-env-changed=BUILD_VERSION");
}
