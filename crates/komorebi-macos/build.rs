fn main() {
    // Link to private frameworks directory for SkyLight APIs
    println!("cargo:rustc-link-search=framework=/System/Library/PrivateFrameworks");

    // SkyLight framework contains SLSDisableUpdate/SLSReenableUpdate for screen update batching
    println!("cargo:rustc-link-lib=framework=SkyLight");
}
