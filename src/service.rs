//! `presence-bridge service install|uninstall`: turn the systemd user service on
//! or off.
//!
//! A packaged binary comes with its unit (krytis ships
//! `/usr/lib/systemd/user/presence-bridge.service`), so `install` only enables
//! it. A binary from `cargo install` or `target/release` has none, so `install`
//! first writes one to `$XDG_CONFIG_HOME/systemd/user/`: this repo's
//! `systemd/presence-bridge.service`, with `ExecStart=` pointed at the running
//! binary and a marker line. `uninstall` disables the unit, and removes the file
//! only if it carries that marker.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub const USAGE: &str = "\
usage: presence-bridge service install|uninstall

  install    enable and start the systemd user service, now and at every
             login; with no unit installed, write one to
             $XDG_CONFIG_HOME/systemd/user/ that runs this binary
  uninstall  stop and disable it, and remove a unit `install` wrote";

const UNIT: &str = "presence-bridge.service";
const TEMPLATE: &str = include_str!("../systemd/presence-bridge.service");
/// First line of a unit `install` wrote. `uninstall` removes only such a file.
const MARKER: &str = "# Written by `presence-bridge service install`; `presence-bridge service uninstall` removes it.";

pub fn main(args: &[String]) -> ExitCode {
    let result = match args {
        [a] if a == "install" => install(),
        [a] if a == "uninstall" => uninstall(),
        [a] if a == "-h" || a == "--help" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("presence-bridge service: {e}");
            ExitCode::from(1)
        }
    }
}

fn install() -> Result<(), String> {
    let path = user_unit()?;
    let write = match owner(&path).map_err(|e| format!("{}: {e}", path.display()))? {
        Owner::Ours => true,
        // The user's own unit, or a mask symlink: theirs to keep.
        Owner::Foreign => false,
        Owner::Absent => fragment()?.is_none(),
    };
    if write {
        let exe = std::env::current_exe().map_err(|e| format!("cannot find this binary: {e}"))?;
        let exe = exe.to_str().ok_or_else(|| format!("{}: path is not UTF-8", exe.display()))?;
        let unit = render(exe)?;
        if fs::read_to_string(&path).ok().as_deref() != Some(unit.as_str()) {
            fs::create_dir_all(path.parent().expect("unit path has a parent"))
                .and_then(|()| fs::write(&path, &unit))
                .map_err(|e| format!("{}: {e}", path.display()))?;
            println!("wrote {} (ExecStart={exe})", path.display());
            systemctl(&["daemon-reload"])?;
        }
    }
    systemctl(&["enable", "--now", UNIT])?;
    let from = fragment()?.unwrap_or_default();
    println!("{UNIT} is enabled and running ({from})");
    Ok(())
}

fn uninstall() -> Result<(), String> {
    let path = user_unit()?;
    let owner = owner(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if fragment()?.is_some() {
        systemctl(&["disable", "--now", UNIT])?;
        println!("{UNIT} is stopped and disabled");
    } else if owner == Owner::Absent {
        println!("{UNIT} is not installed");
    }
    if owner == Owner::Ours {
        fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("removed {}", path.display());
        systemctl(&["daemon-reload"])?;
    }
    Ok(())
}

/// Where systemd reads the user's own units: `$XDG_CONFIG_HOME/systemd/user`.
fn user_unit() -> Result<PathBuf, String> {
    // XDG: a relative XDG_CONFIG_HOME is invalid and ignored.
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|c| c.join("systemd/user").join(UNIT))
        .ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is set".into())
}

#[derive(Debug, PartialEq)]
enum Owner {
    Absent,
    /// `install` wrote it.
    Ours,
    /// Anything else, including a mask symlink to `/dev/null`.
    Foreign,
}

fn owner(path: &Path) -> io::Result<Owner> {
    let meta = match path.symlink_metadata() {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Owner::Absent),
        m => m?,
    };
    if !meta.is_file() {
        return Ok(Owner::Foreign);
    }
    let text = fs::read_to_string(path)?;
    Ok(if text.lines().next() == Some(MARKER) { Owner::Ours } else { Owner::Foreign })
}

/// The unit file systemd loads for presence-bridge, if it finds one anywhere on
/// its search path.
fn fragment() -> Result<Option<String>, String> {
    let out = Command::new("systemctl")
        .args(["--user", "show", "--property=FragmentPath", "--value", UNIT])
        .output()
        .map_err(|e| format!("cannot run systemctl: {e}"))?;
    if !out.status.success() {
        return Err(format!("systemctl --user show {UNIT}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    Ok((!path.is_empty()).then_some(path))
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .map_err(|e| format!("cannot run systemctl: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("systemctl --user {} failed ({status})", args.join(" "))) }
}

/// The repo's unit, marked as ours, with `ExecStart=` running `exe`.
fn render(exe: &str) -> Result<String, String> {
    let exec = exec_path(exe)?;
    let mut unit = format!("{MARKER}\n");
    for line in TEMPLATE.lines() {
        if line.starts_with("ExecStart=") {
            unit.push_str(&format!("ExecStart={exec}\n"));
        } else {
            unit.push_str(line);
            unit.push('\n');
        }
    }
    Ok(unit)
}

/// `exe` as the command of an `ExecStart=` line (systemd.syntax(7),
/// systemd.service(5) § Command lines): `%` and `$` doubled so they are not
/// read as specifiers or variables, and quoted when the path has a space,
/// quote or backslash.
fn exec_path(exe: &str) -> Result<String, String> {
    if !exe.starts_with('/') {
        return Err(format!("{exe}: not an absolute path"));
    }
    if exe.contains(['\n', '\r']) {
        return Err(format!("{exe:?}: path has a line break"));
    }
    let exe = exe.replace('%', "%%").replace('$', "$$");
    if !exe.contains(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '\\')) {
        return Ok(exe);
    }
    Ok(format!("\"{}\"", exe.replace('\\', "\\\\").replace('"', "\\\"")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tempdir;

    #[test]
    fn render_points_the_repo_unit_at_the_binary() {
        let unit = render("/home/u/.cargo/bin/presence-bridge").unwrap();
        let mut lines = unit.lines();
        assert_eq!(lines.next(), Some(MARKER));
        let rest: Vec<_> = lines.collect();
        let template: Vec<_> = TEMPLATE.lines().collect();
        assert_eq!(rest.len(), template.len());
        let changed: Vec<_> = rest.iter().zip(&template).filter(|(a, b)| a != b).collect();
        assert_eq!(changed, [(&"ExecStart=/home/u/.cargo/bin/presence-bridge", &"ExecStart=/usr/bin/presence-bridge")]);
    }

    #[test]
    fn exec_path_escapes_what_systemd_would_expand() {
        assert_eq!(exec_path("/usr/bin/presence-bridge").unwrap(), "/usr/bin/presence-bridge");
        assert_eq!(exec_path("/opt/100%/$HOME/pb").unwrap(), "/opt/100%%/$$HOME/pb");
        assert_eq!(exec_path("/home/u/My Apps/pb").unwrap(), "\"/home/u/My Apps/pb\"");
        assert_eq!(exec_path(r#"/a"b\c/pb"#).unwrap(), r#""/a\"b\\c/pb""#);
        assert!(exec_path("target/release/presence-bridge").is_err());
        assert!(exec_path("/a\nExecStartPre=/bin/evil").is_err());
    }

    #[test]
    fn owner_tells_our_unit_from_anyone_elses() {
        let d = tempdir("service-owner");
        let unit = d.join(UNIT);
        assert_eq!(owner(&unit).unwrap(), Owner::Absent);
        fs::write(&unit, render("/usr/bin/presence-bridge").unwrap()).unwrap();
        assert_eq!(owner(&unit).unwrap(), Owner::Ours);
        fs::write(&unit, TEMPLATE).unwrap();
        assert_eq!(owner(&unit).unwrap(), Owner::Foreign);
        // `systemctl --user mask` puts a /dev/null symlink here.
        fs::remove_file(&unit).unwrap();
        std::os::unix::fs::symlink("/dev/null", &unit).unwrap();
        assert_eq!(owner(&unit).unwrap(), Owner::Foreign);
        // A dangling symlink is still not ours to replace.
        fs::remove_file(&unit).unwrap();
        std::os::unix::fs::symlink(d.join("gone"), &unit).unwrap();
        assert_eq!(owner(&unit).unwrap(), Owner::Foreign);
        fs::remove_dir_all(&d).unwrap();
    }
}
