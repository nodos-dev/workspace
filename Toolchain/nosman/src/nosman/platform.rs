use std::fmt::{Display, Formatter, Error};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    pub arch: String,
    pub os: String,
}

impl Platform {
    pub fn new(arch: String, os: String) -> Platform {
        Platform { arch, os }
    }
    pub fn from_str(s: &str) -> Option<Platform> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return None;
        }
        Some(Platform::new(parts[0].to_string(), parts[1].to_string()))
    }
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        write!(f, "{}-{}", self.arch, self.os)
    }
}

pub fn get_host_platform() -> Platform {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    Platform::new(arch.to_string(), os.to_string())
}