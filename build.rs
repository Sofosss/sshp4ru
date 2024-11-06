fn main() {
    //Operating system check
    let uname_output = std::process::Command::new("uname")
        .arg("-s")
        .output()
        .expect("Failed to execute uname");

    let os_name_lossy = String::from_utf8_lossy(&uname_output.stdout);
    let os_name = os_name_lossy.trim();

    // Kqueue if OS is Darwin or FreeBSD
    if os_name == "Darwin" || os_name == "FreeBSD" {
        println!("cargo:rustc-cfg=feature=\"use_kqueue\"");
    }
}
