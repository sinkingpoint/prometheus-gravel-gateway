use std::{hash::{Hash, BuildHasher, Hasher, BuildHasherDefault}, str::FromStr, io::BufRead};
use trust_dns_resolver::{Resolver, error::ResolveError};
use trust_dns_resolver::Name;
use twox_hash::XxHash64;

struct HashRing<T: Hash, H: BuildHasher> {
    keys: Vec<(u64, T)>,
    hasher: H,
}

fn hash_one<T: Hash, H: BuildHasher>(hasher: &H, val: &T) -> u64 {
    let mut hasher = hasher.build_hasher();
    val.hash(&mut hasher);
    hasher.finish()
}

impl<T: Hash, H: BuildHasher> HashRing<T, H> {
    pub fn new_with_nodes(hasher: H, nodes: impl IntoIterator<Item=T>) -> Self {
        let mut keys: Vec<(u64, T)> = nodes.into_iter().map(|v| (hash_one(&hasher, &v), v)).collect();
        keys.sort_by_key(|&(k, _)| k);

        HashRing {
            keys,
            hasher,
        }
    }

    fn get_key<V: Hash>(&self, val: &V) -> u64 {
        return hash_one(&self.hasher, val);
    }

    pub fn add_node(&mut self, node: T) {
        let key = self.get_key(&node);
        let idx = self.keys.binary_search_by_key(&key, |&(k, _)| k).unwrap_or_else(|idx| idx);
        self.keys.insert(idx, (key, node));
    }

    pub fn get_node_for_val<V: Hash>(&self, val: &V) -> Option<&T> {
        if self.keys.len() == 0 {
            return None;   
        }

        let key = self.get_key(val);
        for k in self.keys.iter() {
            if key >= k.0 {
                return Some(&k.1);
            }
        }

        return Some(&self.keys.get(&self.keys.len() - 1).unwrap().1);
    }
}

pub struct ClusterConfig {
    self_url: String,
    peers: HashRing<String, BuildHasherDefault<XxHash64>>
}

impl ClusterConfig {
    pub fn new_from_static(mut self_url: String, mut peers: Vec<String>) -> ClusterConfig {
        for peer in peers.iter_mut() {
            if !peer.contains("::/") {
                *peer = "http://".to_owned() + &peer;
            }
        }

        if !self_url.contains("::/") {
            self_url = "http://".to_owned() + &self_url;
        }

        let hasher = BuildHasherDefault::<XxHash64>::default();
        let mut peers = HashRing::new_with_nodes(hasher, peers);
        peers.add_node(self_url.clone());
        
        ClusterConfig {
            self_url,
            peers
        }
    }

    pub fn is_self(&self, url: &str) -> bool {
        url == self.self_url
    }

    pub fn new_from_srv(self_url: String, srv: &str) -> Result<ClusterConfig, ResolveError> {
        let mut peers = Vec::new();

        let resolver = Resolver::from_system_conf().unwrap();
        match resolver.srv_lookup(Name::from_str(srv).unwrap()) {
            Ok(records) => {
                for record in records {
                    peers.push(record.target().to_string());
                }
            }
            Err(e) => {
                return Err(e);
            }
        }

        return Ok(ClusterConfig::new_from_static(self_url, peers));
    }

    pub fn new_from_file(self_url: String, path: &str) -> Result<ClusterConfig, std::io::Error> {
        let mut peers = Vec::new();
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            peers.push(line.trim().to_string());
        }

        return Ok(ClusterConfig::new_from_static(self_url, peers));
    }

    pub fn get_peer_for_key<T: Hash>(&self, key: &T) -> Option<&String> {
        self.peers.get_node_for_val(key)
    }
}
