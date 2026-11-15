fn main() {
    println!("cargo:rerun-if-changed=../../assets/branding/muvrix.ico");
    println!("cargo:rerun-if-changed=../../assets/branding/muvrix-logo.png");
    println!("cargo:rerun-if-changed=../../assets/branding/muvrix-logo.svg");

    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=muvrix.rc");
    }
}
