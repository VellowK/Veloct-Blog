fn main() {
    tauri_build::build();
    link_the_windows_resource_into_tests();
}

/// Give the TEST executables the same Windows resource the application binary already gets.
///
/// The resource carries the application MANIFEST, and `tauri-build` links it into `[[bin]]` targets only
/// (`cargo:rustc-link-arg-bins`, emitted from inside `tauri_build::build()` above). A test executable gets
/// none, and without a manifest the loader binds `comctl32.dll` to the version 5.82 copy, which does not
/// export `SetWindowSubclass` / `DefSubclassProc` / `RemoveWindowSubclass` / `TaskDialogIndirect`. A test
/// that builds a real `App` on Tauri's mock runtime pulls those in through the menu code the linker then
/// keeps, so the executable never starts at all — it dies with `STATUS_ENTRYPOINT_NOT_FOUND` (`0xc0000139`)
/// before a single test runs, which is what PROCESSING.md §5.10.2 item 2's four `tauri::State` commands
/// need (`tests/attachments_commands.rs`).
///
/// `cargo:rustc-link-arg-tests` is the ONLY Cargo directive that reaches a test target without also reaching
/// a release artifact, and Cargo applies it to real test targets but NOT to the lib's unit-test harness —
/// which is why those tests are an integration test.
///
/// This is the resource `tauri-build` has already compiled, for TEST targets only, so nothing a release
/// build links changes.
fn link_the_windows_resource_into_tests() {
    if !cfg!(windows) {
        return;
    }
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        // No OUT_DIR means this is not running under Cargo, so there is nothing to point at.
        return;
    };
    let out_dir = std::path::Path::new(&out_dir);
    // `.lib` is what `embed-resource` produces for MSVC, `.o` for the GNU toolchains: whichever exists is
    // the file `tauri-build` just emitted its own `-bins` argument for.
    for name in ["resource.lib", "resource.o"] {
        let resource = out_dir.join(name);
        if resource.is_file() {
            println!("cargo:rustc-link-arg-tests={}", resource.display());
            return;
        }
    }
    // SAID OUT LOUD RATHER THAN SKIPPED, because the failure it would otherwise cause is inscrutable: this
    // file is an UNDOCUMENTED artefact of `tauri-build`/`embed-resource`, so an upstream rename would leave
    // every test target without a manifest and `tests/attachments_commands.rs` would die at LOAD with
    // `0xc0000139 STATUS_ENTRYPOINT_NOT_FOUND`, before a single test ran — which reads like a broken toolchain
    // rather than a missing resource. A warning and not a panic: a release build does not need this at all, and
    // refusing to build the PRODUCT over a TEST-only helper would be the wrong trade.
    println!(
        "cargo:warning=no Windows resource in {} (looked for resource.lib and resource.o): the mock-App \
         integration tests will fail to LOAD with 0xc0000139 until this is fixed - see this file's comment",
        out_dir.display()
    );
}
