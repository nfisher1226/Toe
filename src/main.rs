mod config;
mod time;
mod threadpool;

use {
    chrono::Timelike,
    config::Config,
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
        sync::Mutex,
    },
    sysinfo::{ComponentExt, System, SystemExt},
    time::Time,
    threadpool::ThreadPool,
};

lazy_static! {
    static ref CONFIG: Config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Unable to load config: {e}");
            process::exit(1);
        },
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
        PathBuf::from(&CONFIG.root)
    } else {
        PathBuf::from("/")
    };
    if let Ok(dir) = fs::read_dir(root) {
        for entry in dir {
            if let Ok(entry) = entry {
                let mut path = entry.path();
                path.push(".plan");
                paths.push(path);
            }
        }
    }
    paths
}

fn server_info() -> Result<String, std::fmt::Error> {
    let mut sysinfo = format!("{}\n", CONFIG.server);
    for _ in 0..CONFIG.server.len() {
        write!(sysinfo, "=")?;
    }
    write!(sysinfo, "\n\n")?;
    if CONFIG.stats.users {
        write!(sysinfo, "Users: ")?;
        for path in users().iter() {
            if path.exists() {
                if let Some(name) = path.to_string_lossy().split('/').skip(1).next() {
                    write!(sysinfo, " {}", name)?;
                }
            }
        }
        write!(sysinfo, "\n\n")?;
    }
    let mut sys = SYS.lock().unwrap();
    if CONFIG.stats.uptime {
        write!(sysinfo, "System Status\n-------------\n\n")?;
        sys.refresh_all();
        let current = chrono::Utc::now();
        let uptime = Time::uptime(&sys);
        let users = sys.users().len();
        let load = sys.load_average();
        write!(
            sysinfo,
            "{:02}:{:02}:{:02} up {} days {:02}:{:02}, {} users, load average {} {} {}\n\n",
            current.hour(),
            current.minute(),
            current.second(),
            uptime.days(),
            uptime.hours(),
            uptime.minutes(),
            users,
            load.one,
            load.five,
            load.fifteen,
        )?;
    }
    if CONFIG.stats.cpu {
        let components = sys.components();
        for component in components {
            if component.label().starts_with("Package") {
                writeln!(
                    sysinfo,
                    "{}: +{}°C  (max = +{}°C, critical = +{}°C)",
                    &component.label(),
                    &component.temperature(),
                    &component.max(),
                    &component.critical().unwrap_or(component.max()),
                )?;
            }
        }
        for component in components {
            if component.label().starts_with("Core") {
                writeln!(
                    sysinfo,
                    "{}:       +{}°C  (max = +{}°C, critical = +{}°C)",
                    &component.label(),
                    &component.temperature(),
                    &component.max(),
                    &component.critical().unwrap_or(component.max()),
                )?;
            }
        }
    }
    Ok(sysinfo)
}

fn handle_connection(mut stream: TcpStream) -> std::io::Result<()> {
    let mut buf = [0; 1024];
    stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..]);
    let request: Vec<String> = request.split_whitespace().map(|x| x.to_string()).collect();
    if request.len() > 2 {
        stream.write(b"Malformed response\n")?;
        return Err(Error::new(ErrorKind::Other, "Malformed response"));
    }
    let user = request[0].trim_matches(char::from(0));
    match user {
        "" => {
            match server_info() {
                Ok(info) => {
                    println!("Serving system info request");
                    stream.write(info.as_bytes())?;
                },
                Err(e) => {
                    eprintln!("{e}");
                    return Err(Error::new(
                        ErrorKind::Other,
                        format!("{}", e),
                    ));
                },
            };
        },
        _ => {
            let mut path = PathBuf::from("/");
            path.push(&user);
            path.push(".plan");
            if path.exists() {
                let output = fs::read_to_string(path)?;
                println!("Serving info for user {}.", &user);
                stream.write(format!("{}\n", &output).as_bytes())?;
            } else {
                eprintln!("Request for unknown user {}.", &user);
                stream.write(format!("{}'s not here man.\n", user).as_bytes())?;
            }
        },
    }
    Ok(())
}

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
        println!("Starting toe server at {}:{}...", uptime.hours(), uptime.minutes());
    }
    let user = CONFIG.getpwnam()?;
    let group = CONFIG.getgrnam()?;
    if CONFIG.chroot {
        unix::fs::chroot(&CONFIG.root)?;
    }
    env::set_current_dir("/")?;
    let listener = TcpListener::bind(format!("{}:{}", CONFIG.address, CONFIG.port))?;
    println!("Binding to address {} on port {}.", CONFIG.address, CONFIG.port);
    privdrop(user, group)?;
    if let Ok(mut sys) = SYS.lock() {
        sys.refresh_all();
    }
    println!("Starting up thread pool");
    let threads = NonZeroUsize::new(CONFIG.threads).unwrap();
    let pool = ThreadPool::new(threads);
    println!("Priviledges dropped, listening for incoming connections.");
    for stream in listener.incoming() {
        let stream = stream.unwrap();
        pool.execute(|| {
            if let Err(e) = handle_connection(stream) {
                eprintln!("{e}");
            }
        });
    }
    Ok(())
}
