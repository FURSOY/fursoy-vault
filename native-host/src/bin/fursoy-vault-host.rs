use std::io;

fn main() {
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
