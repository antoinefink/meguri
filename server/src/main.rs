// SPDX-License-Identifier: MIT
use meguri_sync_server::{Config, DataRoot, Error, Result, Service, httpapi};
use std::{net::SocketAddr, path::PathBuf};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("meguri-sync-server: {}", error.code);
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    // Secrets are never accepted on the command line. Positional configuration
    // is intentionally small; deployment/config-file policy is separate.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err(Error::new("invalid_request"));
    }
    let command = &args[0];
    let path = PathBuf::from(&args[1]);
    let listen: SocketAddr = if command == "serve" {
        args.get(3)
            .ok_or(Error::new("invalid_request"))?
            .parse()
            .map_err(|_| Error::field("listen"))?
    } else {
        "127.0.0.1:0".parse().unwrap()
    };
    let mut config = Config::new(&args[2], listen)?;
    let mut rest = if command == "serve" { 4 } else { 3 };
    let mut admin = None;
    let mut reenable = false;
    if command == "recover-admin" {
        admin = Some(args.get(rest).ok_or(Error::new("invalid_request"))?.clone());
        rest += 1;
    }
    while rest < args.len() {
        match args[rest].as_str() {
            "--reenable" if command == "recover-admin" => {
                reenable = true;
                rest += 1;
            }
            "--trusted-peer" if command == "serve" => {
                config.trusted_peers.insert(
                    args.get(rest + 1)
                        .ok_or(Error::new("invalid_request"))?
                        .parse()
                        .map_err(|_| Error::field("trusted_peer"))?,
                );
                rest += 2;
            }
            "--temp-bytes" if command == "serve" => {
                config.temp_bytes = args
                    .get(rest + 1)
                    .ok_or(Error::new("invalid_request"))?
                    .parse()
                    .map_err(|_| Error::field("temp_bytes"))?;
                rest += 2;
            }
            "--native-compressed-bytes" | "--native-decompressed-bytes" if command == "serve" => {
                let limit = args.get(rest + 1).ok_or(Error::new("invalid_request"))?
                    .parse().map_err(|_|Error::field("native_bytes"))?;
                if args[rest] == "--native-compressed-bytes" { config.native_compressed_bytes = limit; }
                else { config.native_decompressed_bytes = limit; }
                rest += 2;
            }
            _ => return Err(Error::new("invalid_request")),
        }
    }
    let create = command == "init";
    let root = tokio::task::spawn_blocking(move || DataRoot::acquire(&path, create))
        .await
        .map_err(|_| Error::new("storage_unavailable"))??;
    match command.as_str() {
        "init" => {
            let (root, code) = tokio::task::spawn_blocking(move || {
                let mut root = root;
                root.init()?;
                let code = root.setup_code()?;
                Ok::<_, Error>((root, code))
            })
            .await
            .map_err(|_| Error::new("storage_unavailable"))??;
            println!(
                "one-time setup code: {code}\nsetup file: {}\nsetup page: {}/setup",
                root.setup_file().display(),
                config.origin()
            );
            Ok(())
        }
        "rotate-setup" => {
            let location = tokio::task::spawn_blocking(move || {
                root.rotate_setup()?;
                Ok::<_, Error>(root.setup_file())
            })
            .await
            .map_err(|_| Error::new("storage_unavailable"))??;
            println!(
                "setup file: {}\nsetup page: {}/setup",
                location.display(),
                config.origin()
            );
            Ok(())
        }
        "recover-admin" => {
            let password = tokio::task::spawn_blocking(protected_password)
                .await
                .map_err(|_| Error::new("internal_error"))??;
            Service::recover_admin(root, admin.unwrap(), password, reenable).await
        }
        "serve" => {
            let service = Service::open(root, config).await?;
            let listener = tokio::net::TcpListener::bind(service.config.listen).await?;
            println!("listening {}", listener.local_addr()?);
            httpapi::serve(listener, service, async {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM handler");
                tokio::select! { _=tokio::signal::ctrl_c()=>{},_=term.recv()=>{} }
            })
            .await?;
            Ok(())
        }
        _ => Err(Error::new("invalid_request")),
    }
}
fn protected_password() -> Result<String> {
    use std::{io::Read, os::fd::AsRawFd};
    let stdin = std::io::stdin();
    let fd = stdin.as_raw_fd();
    // Refuse echoed terminals. Operator may pipe protected input, use a private
    // mode-0600 regular file on stdin, or disable tty echo explicitly.
    // SAFETY: valid stdin descriptor and initialized output storage.
    let tty = unsafe { libc::isatty(fd) } == 1;
    if tty {
        let mut term = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(fd, term.as_mut_ptr()) } != 0 {
            return Err(Error::new("permission_denied"));
        }
        let term = unsafe { term.assume_init() };
        if term.c_lflag & libc::ECHO != 0 {
            return Err(Error::new("permission_denied"));
        }
    } else {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
            return Err(Error::new("permission_denied"));
        }
        let stat = unsafe { stat.assume_init() };
        if stat.st_mode & libc::S_IFMT == libc::S_IFREG
            && (stat.st_uid != unsafe { libc::geteuid() } || stat.st_mode & 0o077 != 0)
        {
            return Err(Error::new("permission_denied"));
        }
    }
    let mut bytes = Vec::with_capacity(1027);
    // Read one byte beyond the largest legal password plus CRLF. Reject
    // before stripping the transport newline, so a legal prefix followed by
    // more input is never silently accepted. Memory and reads remain bounded.
    stdin.lock().take(1027).read_to_end(&mut bytes)?;
    if bytes.len() > 1026 {
        return Err(Error::field("password"));
    }
    let mut value = String::from_utf8(bytes).map_err(|_| Error::field("password"))?;
    // One transport newline, with exact password bytes otherwise preserved.
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    meguri_sync_server::crypto::new_password(&value)?;
    Ok(value)
}
