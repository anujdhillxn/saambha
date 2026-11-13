//! Links gtk4-layer-shell from where scripts/gtk4-layer-shell.sh puts it, as no distro packages it yet.

fn main() {
    let lib = format!("{}/.local/lib", std::env::var("HOME").expect("HOME"));
    println!("cargo:rustc-link-search=native={lib}");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{lib}");
    println!("cargo:rerun-if-env-changed=HOME");
}
