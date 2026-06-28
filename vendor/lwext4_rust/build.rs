use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-env-changed=LWEXT4_TOOLCHAIN_PREFIX");

    let c_path = PathBuf::from("c/lwext4")
        .canonicalize()
        .expect("cannot canonicalize path");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let toolchain_prefix = musl_toolchain_prefix(&arch);
    let lwext4_lib = &format!("lwext4-{arch}");
    {
        let status = Command::new("make")
            .args([
                "musl-generic",
                "-C",
                c_path.to_str().expect("invalid path of lwext4"),
            ])
            .arg(format!("ARCH={arch}"))
            .env("LWEXT4_TOOLCHAIN_PREFIX", &toolchain_prefix)
            .arg(format!(
                "ULIBC={}",
                if env::var("CARGO_FEATURE_STD").is_ok() {
                    "OFF"
                } else {
                    "ON"
                }
            ))
            .arg(format!("OUT_DIR={}", out_dir.display()))
            .status()
            .expect("failed to execute process: make lwext4");
        assert!(status.success());
    }
    {
        let cc = format!("{toolchain_prefix}-gcc");
        let output = Command::new(cc)
            .args(["-print-sysroot"])
            .output()
            .expect("failed to execute process: gcc -print-sysroot");

        let sysroot = PathBuf::from(core::str::from_utf8(&output.stdout).unwrap().trim_end());
        let sysroot_inc = sysroot_include_args(&sysroot);

        generates_bindings_to_rust(&sysroot_inc, &out_dir);
    }

    println!("cargo:rustc-link-lib=static={lwext4_lib}");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rerun-if-changed=c/wrapper.h");
    println!("cargo:rerun-if-changed={}/src", c_path.to_str().unwrap());
}

fn musl_toolchain_prefix(arch: &str) -> String {
    if let Ok(prefix) = env::var("LWEXT4_TOOLCHAIN_PREFIX") {
        return prefix;
    }

    let mut candidates = vec![format!("{arch}-linux-musl")];
    if arch == "riscv64" {
        candidates.push("riscv64-buildroot-linux-musl".to_string());
    }

    candidates
        .into_iter()
        .find(|prefix| {
            Command::new(format!("{prefix}-gcc"))
                .arg("--version")
                .output()
                .is_ok()
        })
        .unwrap_or_else(|| format!("{arch}-linux-musl"))
}

fn sysroot_include_args(sysroot: &Path) -> Vec<String> {
    let mut args = Vec::new();
    let usr_include = sysroot.join("usr/include");
    let include = sysroot.join("include");

    if usr_include.is_dir() {
        args.push(format!("-I{}", usr_include.display()));
    }
    if include.is_dir() {
        args.push(format!("-I{}", include.display()));
    }
    if args.is_empty() {
        args.push(format!("-I{}/include/", sysroot.display()));
    }

    args
}

fn generates_bindings_to_rust(mpaths: &[String], out_dir: &Path) {
    let target = env::var("TARGET").unwrap();
    if target.ends_with("-softfloat") {
        // Clang does not recognize the `-softfloat` suffix
        unsafe { env::set_var("TARGET", target.replace("-softfloat", "")) };
    }

    let mut builder = bindgen::Builder::default()
        .use_core()
        .wrap_unsafe_ops(true)
        // The input header we would like to generate bindings for.
        .header("c/wrapper.h")
        .layout_tests(false);

    for mpath in mpaths {
        builder = builder.clang_arg(mpath);
    }

    let bindings = builder
        //.clang_arg("-I../../ulib/axlibc/include")
        .clang_arg("-I./c/lwext4/include")
        .clang_arg(format!(
            "-I{}/build_musl-generic/include/",
            out_dir.display()
        ))
        // Tell cargo to invalidate the built crate whenever any of the included header files changed.
        .parse_callbacks(Box::new(CustomCargoCallbacks))
        // Finish the builder and generate the bindings.
        .generate()
        .expect("Unable to generate bindings");

    // Restore the original target environment variable
    unsafe { env::set_var("TARGET", target) };

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

#[derive(Debug)]
struct CustomCargoCallbacks;
impl bindgen::callbacks::ParseCallbacks for CustomCargoCallbacks {
    fn header_file(&self, filename: &str) {
        add_include(filename);
    }

    fn include_file(&self, filename: &str) {
        add_include(filename);
    }

    fn read_env_var(&self, key: &str) {
        println!("cargo:rerun-if-env-changed={key}");
    }
}

fn add_include(filename: &str) {
    if !Path::new(filename).ends_with("ext4_config.h") {
        println!("cargo:rerun-if-changed={filename}");
    }
}
