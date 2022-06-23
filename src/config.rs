use {
    serde::Deserialize,
    std::{
        env,
        ffi::CString,
        fs,
        io::{Error, ErrorKind},
    },
};

#[derive(Deserialize)]
pub struct Config {
    /// The name for this server
    pub server: String,
    /// The ip address to bind to
    pub address: String,
    /// The port to run on
    pub port: String,
    /// The user the server should run as
    pub user: String,
    /// The group the server should run as
    pub group: String,
    /// The server "root" directory
    pub root: String,
    /// Whether or not to chroot into the server root
    pub chroot: bool,
    /// The number of worker threads used to server requests
    pub threads: usize,
    pub stats: Stats,
}

#[derive(Default, Deserialize)]
pub struct Stats {
    pub users: bool,
    pub uptime: bool,
    pub kernel: bool,
    pub cpu: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: String::from("localhost"),
            address: String::from("0.0.0.0"),
            port: String::from("79"),
            user: String::from("toe"),
            group: String::from("toe"),
            root: String::from("/srv"),
            threads: 4,
            chroot: true,
            stats: Stats::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, Error> {
        let args: Vec<String> = env::args().collect();
        args.iter().for_each(|arg| {
            println!("Arg: {arg}");
        });
        let raw = fs::read_to_string("/etc/toe.toml")?;
        match toml::from_str(&raw) {
            Ok(c) => Ok(c),
            Err(_) => Err(Error::new(ErrorKind::Other, "Error decoding config file")),
        }
    }

    pub fn getpwnam(&self) -> Result<*mut libc::passwd, Error> {
        let user = CString::new(self.user.as_bytes())?;
        let uid = unsafe { libc::getpwnam(user.as_ptr()) };
        if uid.is_null() {
            eprintln!("Unable to getpwnam of user: {}", &self.user);
            return Err(Error::last_os_error());
        }
        Ok(uid)
    }

    pub fn getgrnam(&self) -> Result<*mut libc::group, Error> {
        let group = CString::new(self.group.as_bytes())?;
        let gid = unsafe { libc::getgrnam(group.as_ptr()) };
        if gid.is_null() {
            eprintln!("Unable to get getgrnam of group: {}", &self.group);
            return Err(Error::last_os_error());
        }
        Ok(gid)
    }
}
