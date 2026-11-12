fn main() {
    // The per-build version is supplied by scripts\build.ps1 through the
    // VIRTUALSSD_BUILD_VERSION environment variable (for example "1.0.0+42").
    // When it is absent — a plain `cargo build` — fall back to the crate
    // version so the build still succeeds and reports something honest.
    let version = std::env::var("VIRTUALSSD_BUILD_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            let pkg = env!("CARGO_PKG_VERSION");
            format!("{pkg}+dev")
        });

    // The build number is the numeric part after '+', used for the Windows
    // file version's fourth component and shown in the UI footer.
    let build = version.split('+').nth(1).unwrap_or("0").to_string();

    println!("cargo:rustc-env=VIRTUALSSD_BUILD_VERSION={version}");
    println!("cargo:rustc-env=VIRTUALSSD_BUILD_NUMBER={build}");
    println!("cargo:rerun-if-env-changed=VIRTUALSSD_BUILD_VERSION");

    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest"));
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
