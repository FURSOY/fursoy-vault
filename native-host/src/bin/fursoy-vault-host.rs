use std::io;
use std::path::Path;

fn main() {
    #[cfg(windows)]
    fursoy_native_host::update::run_startup_hooks();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.as_slice() == ["--check-update"] {
        #[cfg(windows)]
        if let Err(error) = fursoy_native_host::update::check_and_apply() {
            eprintln!("companion update check failed: {error}");
            std::process::exit(1);
        }
        #[cfg(not(windows))]
        eprintln!("this build does not self-update; use your package manager");
        return;
    }
    // Setup launches the installed entry point once after its install callback. Registration is
    // complete and there is no Chrome framing stream, so exit quietly instead of reporting a
    // misleading caller-origin failure to the user.
    if args.is_empty() {
        return;
    }
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
    #[cfg(windows)]
    fursoy_native_host::update::trigger_background_check();
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
