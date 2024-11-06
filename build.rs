
fn main() {
    // Check the operating system
    let uname_output = std::process::Command::new("uname")
        .arg("-s")
        .output()
        .expect("Failed to execute uname");

    let os_name_lossy = String::from_utf8_lossy(&uname_output.stdout);
    let os_name = os_name_lossy.trim();

    // Set a default feature based on the OS
    if os_name == "Darwin" || os_name == "FreeBSD" {
        println!("GIorgossss\n");
        println!("cargo:rustc-cfg=feature=\"use_kqueue\"");
    } 
    else {
        // Default
        // println!("cargo:rustc-cfg=feature=\"use_epoll\"");
    }
}
