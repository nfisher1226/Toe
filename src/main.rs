#![warn(clippy::all, clippy::pedantic)]
mod config;
mod threadpool;
mod time;

use {
    chrono::Timelike,
    config::{Config, Stats},
    lazy_static::lazy_static,
    std::{
        env,
        fmt::Write as _,
        fs,
        io::{Error, ErrorKind, Read, Write},
        net::{TcpListener, TcpStream},
        num::NonZeroUsize,
        os::unix,
        path::PathBuf,
        process,
        sync::{mpsc::channel, Arc, Mutex},
        thread,
    },
    sysinfo::{Component, ComponentExt, System, SystemExt},
    threadpool::ThreadPool,
    time::Time,
};

lazy_static! {
    static ref CONFIG: Config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Unable to load config: {e}");
            process::exit(1);
        }
    };
    static ref SYS: Mutex<System> = Mutex::new(System::new_all());
}

fn privdrop(user: *mut libc::passwd, group: *mut libc::group) -> std::io::Result<()> {
    if unsafe { libc::setgid((*group).gr_gid) } != 0 {
        eprintln!("privdrop: Unable to setgid of group: {}", &CONFIG.group);
        return Err(Error::last_os_error());
    }
    if unsafe { libc::setuid((*user).pw_uid) } != 0 {
        eprintln!("privdrop: Unable to setuid of user: {}", &CONFIG.user);
        return Err(Error::last_os_error());
    }
    Ok(())
}

fn users() -> Vec<PathBuf> {
    let mut paths = vec![];
    let root = if CONFIG.chroot {
        PathBuf::from("/")
    } else {
        PathBuf::from(&CONFIG.root)
    };
    if let Ok(dir) = fs::read_dir(root) {
        for entry in dir.flatten() {
            let mut path = entry.path();
            path.push(".plan");
            paths.push(path);
        }
    }
    paths
}

fn kernel_info(mut sysinfo: String) -> Result<String, std::fmt::Error> {
    let sys = SYS.lock().unwrap();
    if let Some(name) = sys.name() {
        write!(sysinfo, "{name} ")?;
    }
    if let Some(kern) = sys.kernel_version() {
        write!(sysinfo, "{kern} ")?;
    }
    if let Some(os) = sys.os_version() {
        write!(sysinfo, "{os} ")?;
    }
    write!(sysinfo, "\n\n")?;
    Ok(sysinfo)
}

fn user_info(mut sysinfo: String) -> Result<String, std::fmt::Error> {
    write!(sysinfo, "Users: ")?;
    for path in &users() {
        if path.exists() {
            if let Some(name) = path.to_string_lossy().split('/').nth(1) {
                write!(sysinfo, " {name}")?;
            }
        }
    }
    write!(sysinfo, "\n\n")?;
    Ok(sysinfo)
}

fn uptime_info(mut sysinfo: String) -> Result<String, std::fmt::Error> {
    let mut sys = SYS.lock().unwrap();
    write!(sysinfo, "System Status\n-------------\n\n")?;
    sys.refresh_all();
    let current = chrono::Utc::now();
    let uptime = Time::uptime(&sys);
    let users = sys.users().len();
    let load = sys.load_average();
    write!(
        sysinfo,
        "{:02}:{:02}:{:02} up {} days {:02}:{:02}, {users} users, load average {} {} {}\n\n",
        current.hour(),
        current.minute(),
        current.second(),
        uptime.days(),
        uptime.hours(),
        uptime.minutes(),
        load.one,
        load.five,
        load.fifteen,
    )?;
    Ok(sysinfo)
}

fn cpu_info(mut sysinfo: String) -> Result<String, std::fmt::Error> {
    let sys = SYS.lock().unwrap();
    let mut components: Vec<&Component> = sys.components().iter().collect();
    components.iter().try_for_each(|x| {
        if x.label().starts_with("coretemp Core") {
            writeln!(
                sysinfo,
                "{}: +{}°C  (max = +{}°C, critical = +{}°C)",
                &x.label().replace("coretemp ", ""),
                &x.temperature(),
                &x.max(),
                &x.critical().unwrap_or_else(|| x.max()),
            )
        } else if x.label().starts_with("cpu_thermal temp") {
            writeln!(
                sysinfo,
                "{}: +{}°C  (max = +{}°C, critical = +{}°C)",
                &x.label().replace("cpu_thermal temp", "Core "),
                &x.temperature(),
                &x.max(),
                &x.critical().unwrap_or_else(|| x.max()),
            )
        } else {
            Ok(())
        }
    })?;
    let mut cores = components.clone();
    cores.retain(|x| x.label().starts_with("Core"));
    if cores.is_empty() {
        components.retain(|x| x.label().starts_with("CPU"));
        components.iter().try_for_each(|x| {
            writeln!(
                sysinfo,
                "{}:       +{}°C  (max = +{}°C, critical = +{}°C)",
                &x.label(),
                &x.temperature(),
                &x.max(),
                &x.critical().unwrap_or_else(|| x.max()),
            )
        })?;
    } else {
        cores.iter().try_for_each(|x| {
            writeln!(
                sysinfo,
                "{}:       +{}°C  (max = +{}°C, critical = +{}°C)",
                &x.label(),
                &x.temperature(),
                &x.max(),
                &x.critical().unwrap_or_else(|| x.max()),
            )
        })?;
    }
    Ok(sysinfo)
}

fn server_info() -> Result<String, std::fmt::Error> {
    let mut sysinfo = format!("{}\n", CONFIG.server);
    for _ in 0..CONFIG.server.len() {
        write!(sysinfo, "=")?;
    }
    write!(sysinfo, "\n\n")?;
    if CONFIG.stats.contains(&Stats::Kernel) {
        sysinfo = kernel_info(sysinfo)?;
    }
    if CONFIG.stats.contains(&Stats::Users) {
        sysinfo = user_info(sysinfo)?;
    }
    if CONFIG.stats.contains(&Stats::Uptime) {
        sysinfo = uptime_info(sysinfo)?;
    }
    if CONFIG.stats.contains(&Stats::Cpu) {
        sysinfo = cpu_info(sysinfo)?;
    }
    Ok(sysinfo)
}

fn handle_connection(mut stream: TcpStream) -> std::io::Result<()> {
    let mut buf = vec![0; 1024];
    let _len = stream.read(&mut buf)?;
    let request = String::from_utf8(buf).unwrap();
    let request = request.trim_matches(char::from(0)).trim();
    if request.contains(char::is_whitespace) {
        _ = stream.write(b"Malformed response\n")?;
        return Err(Error::new(ErrorKind::Other, "Malformed response"));
    }
    if request.is_empty() {
        match server_info() {
            Ok(info) => {
                println!("Serving system info request");
                _ = stream.write(info.as_bytes())?;
            }
            Err(e) => {
                eprintln!("{e}");
                return Err(Error::new(ErrorKind::Other, format!("{e}")));
            }
        };
    } else {
        let mut path = PathBuf::from("/");
        path.push(request);
        path.push(".plan");
        if path.exists() {
            let output = fs::read_to_string(path)?;
            println!("Serving info for user {request}.");
            _ = stream.write(format!("{output}\n").as_bytes())?;
        } else {
            eprintln!("Request for unknown user {request}.");
            _ = stream.write(format!("{request}'s not here man.\n").as_bytes())?;
        }
    }
    Ok(())
}

#[allow(clippy::similar_names)]
fn main() -> std::io::Result<()> {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    if uid != 0 && gid != 0 {
        eprintln!("Toe must be started as the root user.");
        process::exit(1);
    } else {
        let mut sys = SYS.lock().unwrap();
        sys.refresh_all();
        let uptime = Time::uptime(&sys);
        println!(
            "Starting toe server at {}:{}...",
            uptime.hours(),
            uptime.minutes()
        );
    }
    let user = CONFIG.getpwnam()?;
    let group = CONFIG.getgrnam()?;
    if CONFIG.chroot {
        unix::fs::chroot(&CONFIG.root)?;
    }
    env::set_current_dir("/")?;
    let listener = TcpListener::bind(format!("{}:{}", CONFIG.address, CONFIG.port))?;
    println!(
        "Binding to address {} on port {}.",
        CONFIG.address, CONFIG.port
    );
    privdrop(user, group)?;
    if let Ok(mut sys) = SYS.lock() {
        sys.refresh_all();
    }
    println!("Starting up thread pool");
    let threads = NonZeroUsize::new(CONFIG.threads).unwrap();
    let pool = Arc::new(Mutex::new(ThreadPool::new(threads)));
    println!("Priviledges dropped, listening for incoming connections.");
    {
        let pool = Arc::clone(&pool);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = stream.unwrap();
                if let Ok(pool) = pool.try_lock() {
                    pool.execute(|| {
                        if let Err(e) = handle_connection(stream) {
                            eprintln!("{e}");
                        }
                    });
                }
            }
        });
    }
    let (tx, rx) = channel();
    ctrlc::set_handler(move || {
        tx.send(()).expect("Cannot send termination signal");
    })
    .expect("Cannot set signal handler");
    rx.recv()
        .expect("Could not receive message through channel");
    if let Ok(mut pool) = pool.try_lock() {
        pool.shutdown();
    }
    Ok(())
}
