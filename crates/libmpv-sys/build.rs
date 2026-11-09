use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!("cargo:rerun-if-env-changed=MPV_DIR");
    println!("cargo:rerun-if-env-changed=MPV_LIB_DIR");
    println!("cargo:rerun-if-env-changed=MPV_INCLUDE_DIR");

    // 1. Direct environment overrides (MPV_LIB_DIR or MPV_DIR)
    if let Ok(lib_dir) = env::var("MPV_LIB_DIR") {
        println!("cargo:rustc-link-search=native={lib_dir}");
        println!("cargo:rustc-link-lib=mpv");
        return;
    }

    if let Ok(mpv_dir) = env::var("MPV_DIR") {
        let p = PathBuf::from(&mpv_dir);
        println!("cargo:rustc-link-search=native={}", p.display());
        let lib_dir = p.join("lib");
        if lib_dir.exists() {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
        }
        println!("cargo:rustc-link-lib=mpv");

        let inc_dir = p.join("include");
        if inc_dir.exists() {
            println!("cargo:include={}", inc_dir.display());
        }
        return;
    }

    // 2. Discover and prepend candidate pkg-config directories
    let mut candidate_pkg_paths = Vec::new();

    if let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) {
        let h = PathBuf::from(home);
        candidate_pkg_paths.push(h.join(".local/lib64/pkgconfig"));
        candidate_pkg_paths.push(h.join(".local/lib/pkgconfig"));
    }

    // macOS Homebrew default paths
    candidate_pkg_paths.push(PathBuf::from("/opt/homebrew/lib/pkgconfig"));
    candidate_pkg_paths.push(PathBuf::from("/usr/local/lib/pkgconfig"));

    // Standard Unix paths
    candidate_pkg_paths.push(PathBuf::from("/usr/lib64/pkgconfig"));
    candidate_pkg_paths.push(PathBuf::from("/usr/lib/pkgconfig"));

    let existing_pkg_config = env::var("PKG_CONFIG_PATH").unwrap_or_default();
    let mut new_paths: Vec<String> = candidate_pkg_paths
        .into_iter()
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    if !existing_pkg_config.is_empty() {
        new_paths.push(existing_pkg_config);
    }

    if !new_paths.is_empty() {
        let separator = if cfg!(target_os = "windows") {
            ";"
        } else {
            ":"
        };
        let combined = new_paths.join(separator);
        unsafe {
            env::set_var("PKG_CONFIG_PATH", combined);
        }
    }

    // 3. Probe via pkg-config
    match pkg_config::Config::new()
        .atleast_version("2.6")
        .probe("mpv")
    {
        Ok(library) => {
            for path in library.include_paths {
                println!("cargo:include={}", path.display());
            }
        }
        Err(err) => {
            // Fallback: search known library locations for platform-specific library artifacts
            let mut search_dirs = Vec::new();
            if let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) {
                let h = PathBuf::from(home);
                search_dirs.push(h.join(".local/lib64"));
                search_dirs.push(h.join(".local/lib"));
            }
            search_dirs.push(PathBuf::from(r"C:\mpv-dev"));
            search_dirs.push(PathBuf::from(r"C:\mpv-dev\lib"));
            search_dirs.push(PathBuf::from("/opt/homebrew/lib"));
            search_dirs.push(PathBuf::from("/usr/local/lib"));
            search_dirs.push(PathBuf::from("/usr/lib64"));
            search_dirs.push(PathBuf::from("/usr/lib"));

            let target_names = [
                // Linux SONAMEs
                "libmpv.so.2",
                "libmpv.so",
                // macOS dylibs
                "libmpv.2.dylib",
                "libmpv.dylib",
                // Windows import libraries
                "mpv.lib",
                "libmpv.dll.a",
                "mpv.dll",
            ];

            let mut found_dir = None;
            for dir in &search_dirs {
                if !dir.is_dir() {
                    continue;
                }
                for name in &target_names {
                    if dir.join(name).exists() {
                        found_dir = Some(dir.clone());
                        break;
                    }
                }
                if found_dir.is_some() {
                    break;
                }
            }

            if let Some(dir) = found_dir {
                println!("cargo:rustc-link-search=native={}", dir.display());
                println!("cargo:rustc-link-lib=mpv");
            } else {
                println!("cargo:warning=pkg-config failed to find mpv: {err}. Falling back to default linker search.");
                println!("cargo:rustc-link-lib=mpv");
            }
        }
    }
}
