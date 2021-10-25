use std::{fs::File, io::{self, BufRead, BufReader}, path::PathBuf};

pub trait Authenticator {
    fn authenticate(&self, token: &str) -> bool;
}

#[cfg(feature="auth")]
#[derive(Clone)]
pub struct BasicAuthenticator {
    allowed_hashes: Vec<String>
}

#[cfg(feature="auth")]
impl BasicAuthenticator {
    fn load_from_file(path: PathBuf) -> Result<BasicAuthenticator, io::Error> {
        let mut allowed_hashes = Vec::new();
        for line in BufReader::new(File::open(path)?).lines() {
            match line {
                Ok(line) => allowed_hashes.push(line),
                Err(e) => return Err(e)
            }
        }

        return Ok(BasicAuthenticator {
            allowed_hashes
        });
    }
}

#[cfg(feature="auth")]
impl Authenticator for BasicAuthenticator {
    fn authenticate(&self, header: &str) -> bool {
        use bcrypt::verify;
        // Header is in the format "Basic <token>", so here we extract the second bit
        match header.split_ascii_whitespace().nth(1) {
            Some(token) => {
                for hash in self.allowed_hashes.iter() {
                    return verify(token, hash).unwrap_or(false)
                }

                return false
            }
            None => {
                return false
            }
        }
    }
}

#[cfg(feature="auth")]
pub fn basic_auth(config_file_path: PathBuf) -> Result<BasicAuthenticator, io::Error> {
    BasicAuthenticator::load_from_file(config_file_path)
}

pub struct PassThroughAuthenticator{}

pub fn pass_through_auth() -> PassThroughAuthenticator {
    return PassThroughAuthenticator {}
}

impl Authenticator for PassThroughAuthenticator {
    fn authenticate(&self, _: &str) -> bool {
        true
    }
}