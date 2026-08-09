use std::io;
use std::path::Path;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let [flag, destination] = args.as_slice()
        && flag == "--export-audit"
    {
        if let Err(error) = export_audit(Path::new(destination)) {
            eprintln!("audit export failed: {error}");
            std::process::exit(1);
        }
        println!("Verified audit export written to {}", destination);
        return;
    }
    if let Err(error) = fursoy_native_host::host_loop::validate_caller_origin(&args) {
        eprintln!("native host terminated fail-closed: {error}");
        std::process::exit(1);
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    if let Err(error) = fursoy_native_host::host_loop::run_connection(&mut reader, &mut writer) {
        // stdout must remain a clean Native Messaging byte stream.
        eprintln!("native host terminated fail-closed: {error}");
        std::process::exit(1);
    }
}

fn export_audit(destination: &Path) -> fursoy_native_host::FcpResult<()> {
    let paths = fursoy_native_host::paths::DataPaths::discover()?;
    let _lock = fursoy_native_host::instance_lock::InstanceLock::acquire(&paths.root)?;
    let destination = if destination.is_absolute() {
        destination.to_path_buf()
    } else {
        std::env::current_dir()?.join(destination)
    };
    fursoy_native_host::audit::AuditLogger::export_verified(&paths.audit_directory, &destination)
}
