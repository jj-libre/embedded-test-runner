fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    // The MSVC linker ignores linker scripts, so the two symbols
    // `embedded-test.x` provides are aliased instead. Remove once embedded-test
    // stops requiring the script on std.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!(
            "cargo::rustc-link-arg=/ALTERNATENAME:embedded_test_linker_file_not_added_to_rustflags=__embedded_test_start"
        );
        println!(
            "cargo::rustc-link-arg=/ALTERNATENAME:_embedded_test_setup=__embedded_test_default_setup"
        );
    } else {
        println!("cargo::rustc-link-arg=-Tembedded-test.x");
    }
}
