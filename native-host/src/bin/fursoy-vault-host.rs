use std::io;
use std::path::Path;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let export = match args.as_slice() {
        [flag, destination] if flag == "--export-audit" => Some((None, destination.as_str())),
        [flag, profile_flag, profile_id, destination]
            if flag == "--export-audit" && profile_flag == "--profile" =>
        {
            match profile_id.parse() {
                Ok(profile_id) => Some((Some(profile_id), destination.as_str())),
                Err(_) => {
                    eprintln!("audit export failed: profile id must be a UUID");
                    std::process::exit(1);
                }
            }
        }
        _ => None,
    };
    if let Some((profile_id, destination)) = export {
        if let Err(error) = export_audit(profile_id, Path::new(destination)) {
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

fn export_audit(
    profile_id: Option<uuid::Uuid>,
    destination: &Path,
) -> fursoy_native_host::FcpResult<()> {
    let paths = fursoy_native_host::paths::DataPaths::discover_for_export(profile_id)?;
    let _lock = fursoy_native_host::instance_lock::InstanceLock::acquire(&paths.root)?;
    let destination = if destination.is_absolute() {
        destination.to_path_buf()
    } else {
        std::env::current_dir()?.join(destination)
    };
    fursoy_native_host::audit::AuditLogger::export_verified(&paths.audit_directory, &destination)
}
