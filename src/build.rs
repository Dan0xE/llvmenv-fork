//! Manage LLVM/Clang builds

use glob::glob;
use log::*;
use regex::Regex;
use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

use crate::config::*;
use crate::error::*;

const LLVMENV_FN: &str = ".llvmenv";

#[derive(Debug)]
pub struct Build {
    name: String,             // name and id of build
    prefix: PathBuf,          // the path where the LLVM build realy exists
    llvmenv: Option<PathBuf>, // path of .llvmenv
}

impl Build {
    fn system() -> Self {
        Build {
            name: "system".into(),
            prefix: PathBuf::from("/usr"),
            llvmenv: None,
        }
    }

    pub fn from_path(path: &Path) -> Self {
        let name = path.file_name().unwrap().to_str().unwrap();
        Build {
            name: name.into(),
            prefix: path.to_owned(),
            llvmenv: None,
        }
    }

    pub fn from_name(name: &str) -> Result<Self> {
        if name == "system" {
            return Ok(Self::system());
        }
        Ok(Build {
            name: name.into(),
            prefix: data_dir()?.join(name),
            llvmenv: None,
        })
    }

    pub fn exists(&self) -> bool {
        self.prefix.is_dir()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    pub fn env_path(&self) -> Option<&Path> {
        match self.llvmenv {
            Some(ref path) => Some(path.as_path()),
            None => None,
        }
    }

    pub fn set_global(&self) -> Result<()> {
        self.set_local(&config_dir()?)
    }

    pub fn set_local(&self, path: &Path) -> Result<()> {
        let env = path.join(LLVMENV_FN);
        let mut f = fs::File::create(&env).with(&env)?;
        write!(f, "{}", self.name).with(env)?;
        info!("Write setting to {}", path.display());
        Ok(())
    }

    pub fn archive(&self, verbose: bool) -> Result<()> {
        let filename = format!("{}.tar.xz", self.name);
        Command::new("tar")
            .arg(if verbose { "cvf" } else { "cf" })
            .arg(&filename)
            .arg("--use-compress-prog=pixz")
            .arg(&self.name)
            .current_dir(data_dir()?)
            .check_run()?;
        println!("{}", data_dir()?.join(filename).display());
        Ok(())
    }

    /// Use `llvm-config --version` command
    pub fn version(&self) -> Result<(u32, u32, u32)> {
        let (stdout, _) = Command::new(self.prefix().join("bin/llvm-config"))
            .arg("--version")
            .check_output()?;
        parse_version(&stdout)
    }
}

fn parse_version(version: &str) -> Result<(u32, u32, u32)> {
    let cap = Regex::new(r"(\d+).(\d).(\d)")
        .unwrap()
        .captures(version)
        .ok_or_else(|| Error::invalid_version(version))?;
    let major = cap[1]
        .parse()
        .map_err(|_| Error::invalid_version(version))?;
    let minor = cap[2]
        .parse()
        .map_err(|_| Error::invalid_version(version))?;
    let patch = cap[3]
        .parse()
        .map_err(|_| Error::invalid_version(version))?;
    Ok((major, minor, patch))
}

fn local_builds() -> Result<Vec<Build>> {
    Ok(glob(data_dir()?.join("*/bin").to_str().unwrap())
        .unwrap()
        .filter_map(|path| {
            if let Ok(path) = path {
                path.parent().map(Build::from_path)
            } else {
                None
            }
        })
        .collect())
}

pub fn builds() -> Result<Vec<Build>> {
    let mut bs = local_builds()?;
    bs.sort_by(|a, b| a.name.cmp(&b.name));
    bs.insert(0, Build::system());
    Ok(bs)
}

fn load_local_env(path: &Path) -> Result<Option<Build>> {
    let cand = path.join(LLVMENV_FN);
    if !cand.exists() {
        return Ok(None);
    }
    let mut f = fs::File::open(&cand).with(&cand)?;
    let mut s = String::new();
    f.read_to_string(&mut s).with(cand)?;
    let name = s.trim();
    let mut build = Build::from_name(name)?;
    if build.exists() {
        build.llvmenv = Some(path.into());
        Ok(Some(build))
    } else {
        Ok(None)
    }
}

fn load_global_env() -> Result<Option<Build>> {
    load_local_env(&config_dir()?)
}

pub fn seek_build() -> Result<Build> {
    // Seek .llvmenv from $PWD
    let mut path = env::current_dir().unwrap();
    loop {
        if let Some(mut build) = load_local_env(&path)? {
            build.llvmenv = Some(path.join(LLVMENV_FN));
            return Ok(build);
        }
        path = match path.parent() {
            Some(path) => path.into(),
            None => break,
        };
    }
    // check global setting
    if let Some(mut build) = load_global_env()? {
        build.llvmenv = Some(config_dir()?.join(LLVMENV_FN));
        return Ok(build);
    }
    Ok(Build::system())
}

pub fn expand(archive: &Path, verbose: bool) -> Result<()> {
    if !archive.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Archive doest not found",
        ))
        .with(archive);
    }
    Command::new("tar")
        .arg(if verbose { "xvf" } else { "xf" })
        .arg(archive)
        .current_dir(data_dir()?)
        .check_run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() -> Result<()> {
        // https://github.com/termoshtt/llvmenv/issues/36
        let version =
            "clang version 6.0.1-svn331815-1~exp1~20180510084719.80 (branches/release_60)";
        let (major, minor, patch) = parse_version(version)?;
        assert_eq!(major, 6);
        assert_eq!(minor, 0);
        assert_eq!(patch, 1);
        let version = "clang version 10.0.0 \
            (https://github.com/llvm-mirror/clang 65acf43270ea2894dffa0d0b292b92402f80c8cb)";
        let (major, minor, patch) = parse_version(version)?;
        assert_eq!(major, 10);
        assert_eq!(minor, 0);
        assert_eq!(patch, 0);
        Ok(())
    }
}
