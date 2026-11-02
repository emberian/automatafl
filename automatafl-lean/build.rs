use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let lean_dir = manifest_dir.join("../lean");
    let ir_dir = lean_dir.join(".lake/build/ir/Automatafl");
    let c_dir = manifest_dir.join("c");
    let include_dir = c_dir.join("include");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=c/lean_shim.c");
    println!("cargo:rerun-if-changed=c/include/lean/lean.h");
    println!("cargo:rerun-if-changed=../lean/Automatafl/ListAux.lean");
    println!("cargo:rerun-if-changed=../lean/Automatafl/Base.lean");
    println!("cargo:rerun-if-changed=../lean/Automatafl/Rules.lean");
    println!("cargo:rerun-if-changed=../lean/Automatafl/Termination.lean");
    println!("cargo:rerun-if-changed=../lean/Automatafl/Variants.lean");
    println!("cargo:rerun-if-changed=../lean/Automatafl/PackedBoard.lean");
    println!("cargo:rerun-if-changed=../lean/Automatafl/FFI.lean");

    let c_modules = [
        "ListAux.c",
        "Base.c",
        "Rules.c",
        "Termination.c",
        "Variants.c",
        "PackedBoard.c",
        "FFI.c",
    ];

    // If any emitted C file is missing (e.g. cold checkout), invoke `lake build` in `../lean`.
    let missing_c = c_modules.iter().any(|m| !ir_dir.join(m).exists());
    if missing_c {
        let status = Command::new("lake")
            .arg("build")
            .current_dir(&lean_dir)
            .status()
            .expect("Failed to execute `lake build` in ../lean");
        assert!(status.success(), "`lake build` in ../lean failed");
    }

    let target = env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();
    build
        .include(&include_dir)
        .opt_level(2)
        .warnings(false);

    if target.contains("wasm32") {
        build.define("LEAN_EMSCRIPTEN", None);
        if env::var_os("AR").is_none() && env::var_os("AR_wasm32_unknown_unknown").is_none() {
            if let Ok(output) = Command::new("rustc").arg("--print").arg("sysroot").output() {
                if output.status.success() {
                    let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let host = env::var("HOST").unwrap_or_default();
                    let llvm_ar = PathBuf::from(&sysroot)
                        .join("lib/rustlib")
                        .join(&host)
                        .join("bin/llvm-ar");
                    if llvm_ar.exists() {
                        build.archiver(&llvm_ar);
                    }
                }
            }
        }
    }

    for module in &c_modules {
        let path = ir_dir.join(module);
        assert!(
            path.exists(),
            "Missing Lean-emitted C file: {}",
            path.display()
        );
        build.file(path);
    }

    build.file(c_dir.join("lean_shim.c"));
    build.compile("automatafl_lean_core");
}
