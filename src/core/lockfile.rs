use crate::core::ip::Ip;
use crate::core::manifest::FromFile;
use crate::core::manifest::Id;
use crate::core::source::Source;
use crate::core::uuid::Uuid;
use crate::core::{catalog::CacheSlot, ip::IpSpec};
use crate::core::{
    pkgid::PkgPart,
    version::{self, AnyVersion, Version},
};
use crate::util::anyerror::AnyError;
use crate::util::sha256::Sha256Hash;
use colored::Colorize;
use serde_derive::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::Display;
use std::{path::PathBuf, str::FromStr};

pub const IP_LOCK_FILE: &str = "Orbit.lock";

const LOCK_VERSION: usize = 1;
const LOCK_COMMENT: &str = "This file is auto-generated by Orbit. DO NOT EDIT.";

// define the type to be the most-up-to-date lockfile
pub type LockFile = v1::LockFile;
pub type LockEntry = v1::LockEntry;

#[derive(Debug, PartialEq, Deserialize, Serialize, Clone)]
enum LockVersion {
    V1(v1::LockFile),
}

impl LockVersion {
    /// Casts the out-of-date versions to be the most-up-date data structure
    fn into_latest(self) -> LockFile {
        match self {
            Self::V1(lf) => lf,
        }
    }
}

#[derive(Deserialize)]
struct LockNumber {
    version: usize,
}

impl FromFile for LockFile {
    fn from_file(path: &PathBuf) -> Result<Self, Box<dyn Error>> {
        if path.exists() == true {
            // make sure it is a file
            if path.is_file() == false {
                return Err(AnyError(format!("The lockfile must be a file")))?;
            }
            // open file
            let contents = std::fs::read_to_string(&path)?;

            // grab the version number to determine who to parse
            let data: LockVersion = match toml::from_str::<LockNumber>(&contents)?.version {
                // parse for VERSION 1
                1 => LockVersion::V1(
                    // parse toml syntax
                    match Self::from_str(&contents) {
                        Ok(r) => r,
                        // enter a blank lock file if failed (do not exit)
                        Err(e) => {
                            println!(
                                "{}: failed to parse {} file: {}",
                                "warning".yellow().bold(),
                                IP_LOCK_FILE,
                                e
                            );
                            v1::LockFile::new()
                        }
                    },
                ),
                _ => return Err(AnyError(format!("Unsupported lockfile version")))?,
            };
            Ok(data.into_latest())
        } else {
            Ok(LockFile::new())
        }
    }
}

// version 1 for the lockfile
pub mod v1 {
    use super::*;

    #[derive(Debug, PartialEq, Deserialize, Serialize, Clone)]
    pub struct LockFile {
        // internal number to determine how to parse the current lockfile
        version: usize,
        ip: Vec<LockEntry>,
    }

    impl FromStr for LockFile {
        type Err = toml::de::Error;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            toml::from_str(&s)
        }
    }

    impl LockFile {
        /// Creates a new empty [LockFile].
        pub fn new() -> Self {
            Self {
                version: LOCK_VERSION,
                ip: Vec::new(),
            }
        }

        pub fn unwrap(self) -> Vec<LockEntry> {
            self.ip
        }

        pub fn wrap(reqs: Vec<LockEntry>) -> Self {
            Self {
                version: LOCK_VERSION,
                ip: reqs,
            }
        }

        /// Checks if a lockfile is empty (does not exist).
        pub fn is_empty(&self) -> bool {
            self.ip.len() == 0
        }

        /// Creates a lockfile from a build list.
        pub fn from_build_list(build_list: &mut Vec<&Ip>, root: &Ip) -> Self {
            // sort the build list by pkgid and then version
            build_list.sort_by(|&x, &y| {
                match x
                    .get_man()
                    .get_ip()
                    .get_name()
                    .cmp(y.get_man().get_ip().get_name())
                {
                    std::cmp::Ordering::Less => std::cmp::Ordering::Less,
                    std::cmp::Ordering::Equal => x
                        .get_man()
                        .get_ip()
                        .get_version()
                        .cmp(y.get_man().get_ip().get_version()),
                    std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
                }
            });

            Self {
                version: LOCK_VERSION,
                ip: build_list
                    .into_iter()
                    .map(|ip| LockEntry::from((*ip, *ip == root)))
                    .collect(),
            }
        }

        /// Returns an exact match of `target` and `version` from within the lockfile.
        pub fn get(&self, target: &PkgPart, version: &Version) -> Option<&LockEntry> {
            self.ip
                .iter()
                .find(|&f| &f.name == target && &f.version == version)
        }

        /// Returns the highest compatible version from the lockfile for the given `target`.
        pub fn get_highest(&self, target: &PkgPart, version: &AnyVersion) -> Option<&LockEntry> {
            // collect all versions
            let space: Vec<&Version> = self
                .ip
                .iter()
                .filter_map(|f| {
                    if &f.name == target {
                        Some(&f.version)
                    } else {
                        None
                    }
                })
                .collect();
            match version::get_target_version(&version, &space) {
                Ok(v) => self.ip.iter().find(|f| &f.name == target && f.version == v),
                Err(_) => None,
            }
        }

        pub fn inner(&self) -> &Vec<LockEntry> {
            &self.ip
        }

        /// Writes the [LockFile] data to disk.
        pub fn save_to_disk(&self, dir: &PathBuf) -> Result<(), Box<dyn Error>> {
            // write a file
            std::fs::write(
                dir.join(IP_LOCK_FILE),
                format!("# {}\n{}", LOCK_COMMENT, &self.to_string()),
            )?;
            Ok(())
        }
    }

    impl Display for LockFile {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", toml::to_string_pretty(&self).unwrap())
        }
    }

    #[derive(Debug, PartialEq, Deserialize, Serialize, Clone)]
    pub struct LockEntry {
        name: Id,
        version: Version,
        uuid: Uuid,
        // @note: `sum` is optional because the root package will have its sum omitted
        checksum: Option<Sha256Hash>,
        #[serde(flatten)]
        source: Option<Source>,
        dependencies: Vec<IpSpec>,
    }

    impl From<(&Ip, bool)> for LockEntry {
        fn from(ip: (&Ip, bool)) -> Self {
            let is_root = ip.1;
            let ip = ip.0;
            Self {
                name: ip.get_man().get_ip().get_name().clone(),
                version: ip.get_man().get_ip().get_version().clone(),
                uuid: ip.get_uuid().clone(),
                checksum: if is_root == true {
                    None
                } else {
                    Some(
                        Ip::read_checksum_proof(ip.get_root())
                            .unwrap_or(Ip::compute_checksum(ip.get_root())),
                    )
                },
                source: ip.get_man().get_ip().get_source().cloned(),
                dependencies: match ip.get_man().get_deps_list(is_root).len() {
                    0 => Vec::new(),
                    _ => {
                        let mut result: Vec<IpSpec> = ip
                            .get_man()
                            .get_deps_list(is_root)
                            .into_iter()
                            .map(|e| IpSpec::new(e.0.clone(), e.1.clone()))
                            .collect();
                        result.sort_by(|x, y| match x.get_name().cmp(&y.get_name()) {
                            std::cmp::Ordering::Less => std::cmp::Ordering::Less,
                            std::cmp::Ordering::Equal => x.get_version().cmp(&y.get_version()),
                            std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
                        });
                        result
                    }
                },
            }
        }
    }

    impl LockEntry {
        /// Performs an equality check against a target entry `other`.
        ///
        /// Ignores the checksum comparison because the target ip should not have its
        /// checksum computed in the .lock file.
        pub fn matches_target(&self, other: &LockEntry) -> bool {
            self.get_name() == other.get_name()
                && self.get_version() == other.get_version()
                && self.get_source() == other.get_source()
                && self.get_deps() == other.get_deps()
        }

        pub fn get_deps(&self) -> &Vec<IpSpec> {
            self.dependencies.as_ref()
        }

        pub fn get_sum(&self) -> Option<&Sha256Hash> {
            self.checksum.as_ref()
        }

        pub fn get_uuid(&self) -> &Uuid {
            &self.uuid
        }

        pub fn get_source(&self) -> Option<&Source> {
            self.source.as_ref()
        }

        pub fn get_name(&self) -> &Id {
            &self.name
        }

        pub fn get_version(&self) -> &Version {
            &self.version
        }

        pub fn to_cache_slot_key(&self) -> CacheSlot {
            CacheSlot::new(self.get_name(), self.get_version(), self.get_sum().unwrap())
        }

        pub fn to_ip_spec(&self) -> IpSpec {
            IpSpec::new(self.name.clone(), self.version.clone())
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn to_string() {
            let lock = LockFile {
                version: 1,
                ip: vec![
                    LockEntry {
                        name: Id::from_str("lab1").unwrap(),
                        version: Version::from_str("0.5.0").unwrap(),
                        uuid: Uuid::nil(),
                        checksum: None,
                        source: Some(Source::from_str("https://go1.here").unwrap()),
                        dependencies: vec![
                            IpSpec::new(
                                PkgPart::from_str("lab4").unwrap(),
                                Version::from_str("0.5.19").unwrap(),
                            ),
                            IpSpec::new(
                                PkgPart::from_str("lab2").unwrap(),
                                Version::from_str("1.0.0").unwrap(),
                            ),
                        ],
                    },
                    LockEntry {
                        name: Id::from_str("lab2").unwrap(),
                        version: Version::from_str("1.0.0").unwrap(),
                        uuid: Uuid::nil(),
                        checksum: Some(Sha256Hash::new()),
                        source: Some(Source::from_str("https://go2.here").unwrap()),
                        dependencies: Vec::new(),
                    },
                    LockEntry {
                        name: Id::from_str("lab3").unwrap(),
                        version: Version::from_str("2.3.1").unwrap(),
                        uuid: Uuid::nil(),
                        checksum: Some(Sha256Hash::new()),
                        source: None,
                        dependencies: Vec::new(),
                    },
                    LockEntry {
                        name: Id::from_str("lab4").unwrap(),
                        version: Version::from_str("0.5.19").unwrap(),
                        uuid: Uuid::nil(),
                        checksum: Some(Sha256Hash::new()),
                        source: None,
                        dependencies: vec![IpSpec::new(
                            PkgPart::from_str("lab3").unwrap(),
                            Version::from_str("2.3.1").unwrap(),
                        )],
                    },
                ],
            };
            println!("{}", &lock.to_string());
            assert_eq!(&lock.to_string(), DATA1);
        }

        #[test]
        fn from_str() {
            let lock = LockFile {
                version: 1,
                ip: vec![
                    LockEntry {
                        name: Id::from_str("lab1").unwrap(),
                        version: Version::from_str("0.5.0").unwrap(),
                        checksum: None,
                        uuid: Uuid::nil(),
                        source: Some(Source::from_str("https://go1.here").unwrap()),
                        dependencies: vec![
                            IpSpec::new(
                                PkgPart::from_str("lab4").unwrap(),
                                Version::from_str("0.5.19").unwrap(),
                            ),
                            IpSpec::new(
                                PkgPart::from_str("lab2").unwrap(),
                                Version::from_str("1.0.0").unwrap(),
                            ),
                        ],
                    },
                    LockEntry {
                        name: Id::from_str("lab2").unwrap(),
                        version: Version::from_str("1.0.0").unwrap(),
                        uuid: Uuid::nil(),
                        checksum: Some(Sha256Hash::new()),
                        source: Some(Source::from_str("https://go2.here").unwrap()),
                        dependencies: Vec::new(),
                    },
                    LockEntry {
                        name: Id::from_str("lab3").unwrap(),
                        version: Version::from_str("2.3.1").unwrap(),
                        uuid: Uuid::nil(),
                        checksum: Some(Sha256Hash::new()),
                        source: None,
                        dependencies: Vec::new(),
                    },
                    LockEntry {
                        name: Id::from_str("lab4").unwrap(),
                        version: Version::from_str("0.5.19").unwrap(),
                        uuid: Uuid::nil(),
                        checksum: Some(Sha256Hash::new()),
                        source: None,
                        dependencies: vec![IpSpec::new(
                            PkgPart::from_str("lab3").unwrap(),
                            Version::from_str("2.3.1").unwrap(),
                        )],
                    },
                ],
            };
            assert_eq!(&LockFile::from_str(&DATA1).unwrap(), &lock);
        }

        const DATA1: &str = r#"version = 1

[[ip]]
name = "lab1"
version = "0.5.0"
uuid = "00000000-0000-0000-0000-000000000000"
url = "https://go1.here"
dependencies = [
    "lab4:0.5.19",
    "lab2:1.0.0",
]

[[ip]]
name = "lab2"
version = "1.0.0"
uuid = "00000000-0000-0000-0000-000000000000"
checksum = "0000000000000000000000000000000000000000000000000000000000000000"
url = "https://go2.here"
dependencies = []

[[ip]]
name = "lab3"
version = "2.3.1"
uuid = "00000000-0000-0000-0000-000000000000"
checksum = "0000000000000000000000000000000000000000000000000000000000000000"
dependencies = []

[[ip]]
name = "lab4"
version = "0.5.19"
uuid = "00000000-0000-0000-0000-000000000000"
checksum = "0000000000000000000000000000000000000000000000000000000000000000"
dependencies = ["lab3:2.3.1"]
"#;
    }
}
