//! Optional hardware helpers (AGENTS.md rule 5). Sonomux needs none of them; when a device is
//! plugged in and the tool that drives it is missing, the Devices screen offers to install it.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

/// How Arch provides a helper.
enum ArchPkg {
    /// Official repos: installed with a graphical password prompt.
    Repo(&'static str),
    /// AUR: needs an AUR helper in a terminal.
    Aur(&'static str),
}

/// A macOS installer pinned to one upstream release. Upstream packages are unsigned, so the
/// SHA-256 recorded here is the only check; bump version and both hashes together.
struct MacPkg {
    repo: &'static str,
    version: &'static str,
    sha256_arm: &'static str,
    sha256_intel: &'static str,
}

pub struct Helper {
    /// Hardware it serves, as shown to the user.
    pub device: &'static str,
    usb_vendor: u16,
    pub name: &'static str,
    /// Executables that mean it is installed.
    binaries: &'static [&'static str],
    mac_app: Option<&'static str>,
    homepage: &'static str,
    arch: ArchPkg,
    /// Pinned macOS .pkg release, if any.
    mac_pkg: Option<MacPkg>,
}

pub const HELPERS: &[Helper] = &[
    Helper {
        device: "TC-Helicon GoXLR",
        usb_vendor: 0x1220,
        name: "GoXLR Utility",
        binaries: &["goxlr-daemon"],
        mac_app: Some("/Applications/GoXLR Utility.app"),
        homepage: "https://github.com/GoXLR-on-Linux/goxlr-utility",
        arch: ArchPkg::Aur("goxlr-utility"),
        mac_pkg: Some(MacPkg {
            repo: "GoXLR-on-Linux/goxlr-utility",
            version: "1.2.4",
            sha256_arm: "2799d0861592e6bbd4fc21b4d6bf9c7c475c9306785c9ae61da5ee9f8da50468",
            sha256_intel: "392ed62e1a2df9862336b20420baf8c40b988538851b88a778fe1a0c2a858ec1",
        }),
    },
    Helper {
        device: "SteelSeries headset",
        usb_vendor: 0x1038,
        name: "headsetcontrol",
        binaries: &["headsetcontrol"],
        mac_app: None,
        homepage: "https://github.com/Sapd/HeadsetControl",
        arch: ArchPkg::Repo("headsetcontrol"),
        mac_pkg: None,
    },
];

/// USB vendor IDs from `ioreg -p IOUSB -l` output ("idVendor" = 4640).
pub fn parse_ioreg(out: &str) -> HashSet<u16> {
    out.lines()
        .filter_map(|l| l.split("\"idVendor\" = ").nth(1))
        .filter_map(|v| v.trim().parse().ok())
        .collect()
}

/// Vendor IDs of every connected USB device.
pub fn usb_vendors() -> HashSet<u16> {
    if cfg!(target_os = "macos") {
        Command::new("ioreg")
            .args(["-p", "IOUSB", "-l", "-w0"])
            .output()
            .map(|o| parse_ioreg(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    } else {
        std::fs::read_dir("/sys/bus/usb/devices")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path().join("idVendor")).ok())
            .filter_map(|v| u16::from_str_radix(v.trim(), 16).ok())
            .collect()
    }
}

/// Find an executable on PATH, plus the usual install dirs (a GUI started from Finder or a
/// launcher often has a minimal PATH).
fn which(bin: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    let found = path
        .split(':')
        .chain(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"])
        .map(|d| Path::new(d).join(bin))
        .find(|p| p.is_file());
    found
}

pub fn installed(h: &Helper) -> bool {
    h.mac_app
        .is_some_and(|a| cfg!(target_os = "macos") && Path::new(a).exists())
        || h.binaries.iter().any(|b| which(b).is_some())
}

/// What the install button does here, as its label.
pub fn action_label(h: &Helper) -> String {
    let direct = if cfg!(target_os = "macos") {
        h.mac_pkg.is_some()
    } else {
        which("pacman").is_some()
    };
    if direct {
        format!("Install {}", h.name)
    } else {
        format!("Get {}…", h.name)
    }
}

/// Helpers for hardware that is plugged in now: (index into HELPERS, installed).
pub fn detect() -> Vec<(usize, bool)> {
    let vendors = usb_vendors();
    HELPERS
        .iter()
        .enumerate()
        .filter(|(_, h)| vendors.contains(&h.usb_vendor))
        .map(|(i, h)| (i, installed(h)))
        .collect()
}

fn open_url(url: &str) -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(opener)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Install a helper the most direct way this system allows. Blocking: downloads may take a
/// moment, so call it off the UI thread. Hands over to the system's own installer or password
/// prompt where one is needed.
pub fn install(h: &Helper) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let Some(mp) = &h.mac_pkg else {
            return open_url(h.homepage);
        };
        let pkg = match download_mac_pkg(mp) {
            Ok(p) => p,
            Err(e) => {
                // Let the user fetch it manually; still report why we didn't.
                let _ = open_url(&format!(
                    "https://github.com/{}/releases/tag/v{}",
                    mp.repo, mp.version
                ));
                return Err(e);
            }
        };
        // Installer.app asks for the admin password itself.
        return Command::new("open")
            .arg(&pkg)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string());
    }
    if which("pacman").is_none() {
        return open_url(h.homepage);
    }
    match h.arch {
        ArchPkg::Repo(pkg) => Command::new("pkexec")
            .args(["pacman", "-S", "--needed", "--noconfirm", pkg])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string()),
        ArchPkg::Aur(pkg) => {
            let Some(aur) = ["yay", "paru"].into_iter().find(|b| which(b).is_some()) else {
                return open_url(&format!("https://aur.archlinux.org/packages/{}", pkg));
            };
            // AUR builds are interactive: run in the user's terminal.
            let term = ["xdg-terminal-exec", "alacritty", "kitty", "foot", "konsole"]
                .into_iter()
                .find(|t| which(t).is_some())
                .ok_or("no terminal found")?;
            let mut cmd = Command::new(term);
            if term != "xdg-terminal-exec" {
                cmd.arg("-e");
            }
            cmd.args([aur, "-S", pkg])
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    }
}

/// The hex digest from `shasum -a 256 <file>` output ("<hex>  <path>").
fn parse_shasum(out: &str) -> Option<String> {
    let tok = out.split_whitespace().next()?;
    (tok.len() == 64 && tok.bytes().all(|b| b.is_ascii_hexdigit())).then(|| tok.to_lowercase())
}

/// Download the pinned release's macOS .pkg for this Mac's CPU and verify its SHA-256;
/// returns its path. A file that fails the check is deleted.
fn download_mac_pkg(mp: &MacPkg) -> Result<std::path::PathBuf, String> {
    let (suffix, want) = if cfg!(target_arch = "aarch64") {
        ("m1", mp.sha256_arm)
    } else {
        ("intel", mp.sha256_intel)
    };
    let name = format!("goxlr-utility-macos-{}-{}.pkg", mp.version, suffix);
    let url = format!(
        "https://github.com/{}/releases/download/v{}/{}",
        mp.repo, mp.version, name
    );
    let dest = std::env::temp_dir().join(&name);
    let ok = Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--max-time", "300", "-o"])
        .arg(&dest)
        .arg(&url)
        .status()
        .map_err(|e| e.to_string())?
        .success();
    if !ok {
        let _ = std::fs::remove_file(&dest);
        return Err(format!("download of {} failed", name));
    }
    let got = Command::new("shasum")
        .args(["-a", "256"])
        .arg(&dest)
        .output()
        .ok()
        .and_then(|o| parse_shasum(&String::from_utf8_lossy(&o.stdout)));
    if got.as_deref() != Some(want) {
        let _ = std::fs::remove_file(&dest);
        return Err(format!(
            "{} download failed its checksum; not installing",
            name
        ));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ioreg_vendor_ids() {
        let out = "  |   \"idVendor\" = 4640\n  \"idProduct\" = 36836\n    \"idVendor\" = 4152\n";
        let v = parse_ioreg(out);
        assert!(v.contains(&0x1220) && v.contains(&0x1038));
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn helpers_are_for_distinct_hardware() {
        let vendors: HashSet<u16> = HELPERS.iter().map(|h| h.usb_vendor).collect();
        assert_eq!(vendors.len(), HELPERS.len());
        assert!(HELPERS.iter().all(|h| !h.binaries.is_empty()));
    }

    const HASH: &str = "2799d0861592e6bbd4fc21b4d6bf9c7c475c9306785c9ae61da5ee9f8da50468";

    #[test]
    fn parses_shasum_output() {
        assert_eq!(
            parse_shasum(&format!("{}  /tmp/x.pkg\n", HASH)),
            Some(HASH.to_string())
        );
        assert_eq!(
            parse_shasum(&format!("{}  /tmp/x.pkg", HASH.to_uppercase())),
            Some(HASH.to_string())
        );
        assert_eq!(parse_shasum(""), None);
        assert_eq!(parse_shasum("nothex  /tmp/x"), None);
        assert_eq!(parse_shasum(&format!("{}  /tmp/x", &HASH[..63])), None);
    }

    #[test]
    fn goxlr_pin_is_well_formed() {
        let mp = HELPERS[0].mac_pkg.as_ref().unwrap();
        assert!(!mp.version.is_empty());
        for h in [mp.sha256_arm, mp.sha256_intel] {
            assert!(h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod live {
    /// Network: picks and downloads the GoXLR Utility .pkg for this Mac. Run with --ignored.
    #[test]
    #[ignore]
    fn downloads_goxlr_utility_pkg() {
        let mp = super::HELPERS[0].mac_pkg.as_ref().unwrap();
        let pkg = super::download_mac_pkg(mp).unwrap();
        let name = pkg.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            name == "goxlr-utility-macos-1.2.4-m1.pkg"
                || name == "goxlr-utility-macos-1.2.4-intel.pkg",
            "{}",
            name
        );
        assert!(std::fs::metadata(&pkg).unwrap().len() > 1_000_000);
        let _ = std::fs::remove_file(pkg);
    }
}
