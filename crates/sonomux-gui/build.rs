// Pick the platform's look for Slint's standard widgets: macOS controls on macOS; elsewhere
// `native` (real Qt widgets when Qt is installed, else Fluent).
fn main() {
    let style = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => "cupertino",
        _ => "native",
    };
    // SONOMUX_STYLE overrides, e.g. to preview another platform's look.
    println!("cargo:rerun-if-env-changed=SONOMUX_STYLE");
    let style = std::env::var("SONOMUX_STYLE").unwrap_or_else(|_| style.to_string());
    let config = slint_build::CompilerConfiguration::new().with_style(style);
    slint_build::compile_with_config("ui/app.slint", config).expect("compile ui/app.slint");
}
