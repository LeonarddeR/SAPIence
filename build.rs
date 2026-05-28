use std::{
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const NVDA_CONTROLLER_VERSION: &str = "2026.1.1";
const URL_FMT: &str =
    "https://download.nvaccess.org/releases/{ver}/nvda_{ver}_controllerClient.zip";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SAPIENCE_NVDA_CONTROLLER_DIR");

    let arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        other => panic!("Unsupported target arch: {other}"),
    };

    let root = if let Ok(dir) = env::var("SAPIENCE_NVDA_CONTROLLER_DIR") {
        PathBuf::from(dir)
    } else {
        ensure_downloaded(NVDA_CONTROLLER_VERSION, arch)
            .expect("failed to acquire NVDA controller client")
    };

    let arch_dir = root
        .join(arch)
        .canonicalize()
        .expect("arch dir not found in controller client");

    println!("cargo:rustc-link-search=native={}", arch_dir.display());
    println!("cargo:rustc-link-lib=nvdaControllerClient");
    println!("cargo:rustc-link-arg=/DELAYLOAD:nvdaControllerClient.dll");
    println!("cargo:rustc-link-lib=delayimp");

    set_bundled_libclang();

    let header = arch_dir.join("nvdaController.h");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindgen::Builder::default()
        .header(header.to_string_lossy())
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_function("nvdaController_.+")
        .prepend_enum_name(false)
        .must_use_type("error_status_t")
        .generate()
        .expect("bindgen failed")
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("write bindings");

    // Copy the runtime DLL next to the eventual cdylib output so cargo run /
    // tests / examples find it without extra plumbing.
    if let Some(profile_dir) = target_profile_dir() {
        let dest = profile_dir.join("nvdaControllerClient.dll");
        let _ = fs::create_dir_all(&profile_dir);
        let _ = fs::copy(arch_dir.join("nvdaControllerClient.dll"), dest);
    }
}

fn set_bundled_libclang() {
    use std::process::Command;

    // Skip if already set by caller.
    if env::var_os("LIBCLANG_PATH").is_some() || env::var_os("CLANG_PATH").is_some() {
        return;
    }

    // Ask vswhere for the VS install root that has the LLVM/Clang component.
    let vswhere =
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
    if !vswhere.is_file() {
        return;
    }

    let out = Command::new(&vswhere)
        .args([
            "-latest",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Llvm.Clang",
            "-property",
            "installationPath",
        ])
        .output()
        .ok();
    let install_root = match out.and_then(|o| String::from_utf8(o.stdout).ok()) {
        Some(s) if !s.trim().is_empty() => PathBuf::from(s.trim().to_owned()),
        _ => return,
    };

    // VS ships arch-specific LLVM trees under VC\Tools\Llvm\{x64,ARM64,bin}.
    // Pick the subtree that matches the build HOST (bindgen runs on host, not target).
    let host_arch = std::env::var("HOST").unwrap_or_default();
    let llvm_subdir = match host_arch.split('-').next().unwrap_or("") {
        "x86_64" => "x64",
        "aarch64" => "ARM64",
        _ => "bin",
    };
    let bin = install_root
        .join("VC")
        .join("Tools")
        .join("Llvm")
        .join(llvm_subdir)
        .join("bin");
    if !bin.is_dir() {
        return;
    }

    let clang = bin.join("clang.exe");
    if clang.is_file() {
        // SAFETY: build scripts are single-threaded before proc macros run.
        unsafe { env::set_var("CLANG_PATH", &clang) };
    }
    unsafe { env::set_var("LIBCLANG_PATH", &bin) };
}

fn ensure_downloaded(version: &str, arch: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let dest = out_dir.join("nvda-controller-client").join(version);
    if dest.join(arch).join("nvdaController.h").is_file() {
        return Ok(dest);
    }
    fs::create_dir_all(&dest)?;
    let url = URL_FMT.replace("{ver}", version);
    println!("cargo:warning=Downloading {url}");
    let bytes = ureq::get(&url).call()?.body_mut().read_to_vec()?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let outpath = match entry.enclosed_name() {
            Some(p) => dest.join(p),
            None => continue,
        };
        if entry.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = fs::File::create(&outpath)?;
            std::io::copy(&mut entry, &mut f)?;
        }
    }
    Ok(dest)
}

fn target_profile_dir() -> Option<PathBuf> {
    // OUT_DIR is e.g. .../target/<triple>/<profile>/build/<crate>-<hash>/out
    // Walk up to <profile>/.
    let out_dir = PathBuf::from(env::var("OUT_DIR").ok()?);
    out_dir.ancestors().nth(3).map(Path::to_path_buf)
}
