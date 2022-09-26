use std::{fs::File, io::{self, BufRead, BufReader}, path::PathBuf};

pub trait Authenticator {
    fn authenticate(&self, token: &str) -> Result<bool, anyhow::Error>;
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
    fn authenticate(&self, header: &str) -> Result<bool, anyhow::Error> {
        use bcrypt::verify;
        // Header is in the format "Basic <token>", so here we extract the second bit
        match header.split_ascii_whitespace().nth(1) {
            Some(token) => {
                let token: Option<String> = match base64::decode(token) {
                    Ok(token_bytes) => {
                        match String::from_utf8(token_bytes) {
                            // If we have a valid utc-8 base64 auth, split it on the : (format is username:password), and take the second 
                            // part (i.e. just take the password).
                            Ok(token_str) if token_str.contains(':') => token_str.split(':').nth(1).map_or(None, |s| Some(s.to_owned())),

                            // If we fail do decode it as a valid utf-8 basic auth header, for whatever reason, treat it as plain text
                            Ok(token_str) => Some(token_str),
                            Err(_) => Some(token.to_owned())
                        }
                    },

                    // For backwards compatibility reasons, if we fail to base64 decode the header then we
                    // still accept it as plain text
                    Err(_) => Some(token.to_owned())
                };

                if let Some(token) = token {
                    for hash in self.allowed_hashes.iter() {
                        return Ok(verify(token, hash).unwrap_or(false))
                    }
                }

                Ok(false)
            }
            None => Ok(false)
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
    fn authenticate(&self, _: &str) -> Result<bool, anyhow::Error> {
        Ok(true)
    }
}
