#[cfg(target_os = "windows")]
pub fn enforce_single_instance() {
    let instance =
        single_instance::SingleInstance::new("work-notes-a1b2c3d4").unwrap();
    if !instance.is_single() {
        std::process::exit(0);
    }
    std::mem::forget(instance);
}

#[cfg(not(target_os = "windows"))]
pub fn enforce_single_instance() {}
