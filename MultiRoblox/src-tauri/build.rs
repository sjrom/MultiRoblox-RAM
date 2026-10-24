fn main() {
    println!("cargo:rerun-if-changed=resources/RobloxNative.cs");
    let win = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let csc64 = format!(r"{win}\Microsoft.NET\Framework64\v4.0.30319\csc.exe");
    let csc32 = format!(r"{win}\Microsoft.NET\Framework\v4.0.30319\csc.exe");
    let csc = if std::path::Path::new(&csc64).exists() { csc64 } else { csc32 };
    let status = std::process::Command::new(csc)
        .args([
            "/nologo",
            "/optimize+",
            "/platform:x64",
            "/target:exe",
            "/r:System.Drawing.dll",
            r"/out:resources\RobloxNative.exe",
            r"resources\RobloxNative.cs",
        ])
        .status()
        .expect("failed to run csc.exe");
    assert!(status.success(), "csc.exe failed to compile RobloxNative.cs");
    tauri_build::build()
}
