//! An unpacked NVIDIA driver package: its components (top-level folders, each holding NVIDIA installer packages
//! described by `.nvi` manifests), what each is for, the presets, dependency resolution from the manifests
//! themselves, and building a stripped copy.
//!
//! Only whole optional components are left out, and `setup.cfg` loses just the file references that pointed
//! into them. NVIDIA's signed files (the driver's INF, catalog and binaries) are never changed, so Windows
//! verifies the driver exactly as it would the original package.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// The graphics driver and what it can't work without. Never removed.
    Core,
    /// Installs only on PCs with the matching hardware, so it's always kept.
    Hardware,
    /// A feature some games or setups use: kept by the gaming presets.
    Feature,
    /// The NVIDIA App and the services, overlay, recorder and telemetry it brings.
    App,
    /// Not in Kestrel's list (a newer driver): kept until its purpose is known.
    Unknown,
}

/// What Kestrel knows about an NVIDIA installer package.
pub struct Known {
    pub title: &'static str,
    pub what: &'static str,
    pub role: Role,
    /// What it runs on the PC once installed; "" when it only takes disk space until something uses it.
    pub runs: &'static str,
    /// What you lose without it.
    pub lose: &'static str,
    pub telemetry: bool,
}

/// Kestrel's descriptions, by NVIDIA package name (stable across driver versions). Anything else is Unknown.
pub fn known(package: &str) -> Option<Known> {
    let k = |title, what, role, runs, lose, telemetry| Some(Known { title, what, role, runs, lose, telemetry });
    match package {
        "Display.Driver" => k(
            "Graphics driver",
            "The driver itself, with the NVIDIA Control Panel, DirectX, Vulkan and OpenGL, DLSS (including Frame Generation and Ray Reconstruction), Reflex, Low Latency Mode, G-SYNC, NVENC/NVDEC, shader cache and per-game profiles.",
            Role::Core,
            "The NVIDIA Display Container service, which the Control Panel needs.",
            "Everything: this is the driver.",
            false,
        ),
        "MSVCRuntime2019" | "MSVCRuntime2017" => k(
            "Microsoft Visual C++ runtime",
            "Libraries NVIDIA's components are built with.",
            Role::Core,
            "",
            "Driver components can fail to start.",
            false,
        ),
        "NVPCF" => k(
            "Platform Controller Framework",
            "Lets supported laptops share power between processor and graphics card (Dynamic Boost). Installs only where that hardware exists.",
            Role::Hardware,
            "",
            "Dynamic Boost on supported laptops.",
            false,
        ),
        "USBC" => k(
            "USB-C controller driver",
            "For RTX 20 cards with a USB-C (VirtualLink) port. Installs only where that port exists.",
            Role::Hardware,
            "",
            "The graphics card's USB-C port.",
            false,
        ),
        "Display.PhysX" => k(
            "PhysX System Software",
            "GPU physics for the games that use NVIDIA PhysX.",
            Role::Feature,
            "",
            "Older games with GPU PhysX (Borderlands 2, the Batman: Arkham games, Mirror's Edge) run physics on the processor or won't start.",
            false,
        ),
        "HDAudio.Driver" => k(
            "HD Audio driver",
            "Sound through HDMI or DisplayPort, for monitor or TV speakers.",
            Role::Feature,
            "",
            "Sound through HDMI or DisplayPort.",
            false,
        ),
        "NvDLISR" => k(
            "Deep-learning image scaling model",
            "NVIDIA's AI scaling model (nvngx_dlisr), installed alongside the NVIDIA App. Kept by default in case you use DLDSR in the Control Panel.",
            Role::Feature,
            "",
            "Possibly the DLDSR factors in the Control Panel's DSR setting.",
            false,
        ),
        "Display.NvApp" => k(
            "NVIDIA App",
            "NVIDIA's app for driver updates, game settings, the overlay and recording.",
            Role::App,
            "The app, its overlay and background processes, and daily update-check tasks.",
            "The NVIDIA App (drivers then come from Kestrel, NVIDIA's website or Windows Update).",
            false,
        ),
        "Display.NvApp.MessageBus" => k(
            "NVIDIA App message bus",
            "How the NVIDIA App's parts talk to each other.",
            Role::App,
            "A background service component.",
            "Nothing outside the NVIDIA App.",
            false,
        ),
        "Display.NvApp.NvBackend" => k(
            "NVIDIA App backend",
            "Scans your games for the App's optimal settings.",
            Role::App,
            "A background process that scans game folders.",
            "The App's one-click game settings.",
            false,
        ),
        "Display.NvApp.NvCPL" => k(
            "NVIDIA App settings module",
            "The NVIDIA App's settings pages. The classic NVIDIA Control Panel ships inside the driver and stays.",
            Role::App,
            "",
            "Nothing outside the NVIDIA App.",
            false,
        ),
        "NvContainer" | "NvContainer.LocalSystem" | "NvContainer.Session" | "NvContainer.User" | "NvContainer.AIUser"
        | "NvPlugin.Watchdog" => k(
            "NVIDIA App container services",
            "Background services that host the NVIDIA App's plug-ins.",
            Role::App,
            "Background services running all the time.",
            "Nothing outside the NVIDIA App.",
            false,
        ),
        "NvTelemetry" => k(
            "NVIDIA telemetry",
            "Usage and crash reporting for NVIDIA's apps.",
            Role::App,
            "Runs inside the container services and sends data to NVIDIA.",
            "Nothing you use.",
            true,
        ),
        "ShadowPlay" => k(
            "ShadowPlay (recording and overlay)",
            "Instant Replay, recording and the in-game overlay. NVENC itself is part of the driver, so OBS and Discord keep hardware encoding.",
            Role::App,
            "An overlay that hooks into games, and Instant Replay recording when on.",
            "NVIDIA's own recorder and overlay.",
            false,
        ),
        "VirtualAudio.Driver" => k(
            "NVIDIA Virtual Audio",
            "A virtual sound device ShadowPlay records through.",
            Role::App,
            "An extra audio device.",
            "Nothing outside ShadowPlay.",
            false,
        ),
        "FrameViewSdk" => k(
            "FrameView SDK",
            "Frame-rate measurement service for the NVIDIA App's performance overlay.",
            Role::App,
            "A background service.",
            "The App's performance overlay.",
            false,
        ),
        _ => None,
    }
}

/// One top-level folder of the package.
#[derive(Clone, Debug)]
pub struct Component {
    pub folder: String,
    /// NVIDIA package names declared in its manifests (template names left out).
    pub packages: Vec<String>,
    /// Manifest file names without extension, lower-case and without dots: matches template-named packages
    /// such as `NvContainer.LocalSystem` (NvContainerLocalSystem.nvi).
    stems: Vec<String>,
    /// Package names its manifests require.
    pub requires: Vec<String>,
    pub bytes: u64,
}

impl Component {
    /// The package Kestrel describes it by: the first one it knows.
    pub fn main(&self) -> &str {
        self.packages.iter().find(|p| known(p).is_some()).or(self.packages.first()).map_or(&self.folder, String::as_str)
    }

    pub fn known(&self) -> Option<Known> {
        known(self.main())
    }

    pub fn role(&self) -> Role {
        self.known().map_or(Role::Unknown, |k| k.role)
    }

    fn provides(&self, package: &str) -> bool {
        let norm = norm(package);
        self.packages.iter().any(|p| p.eq_ignore_ascii_case(package)) || self.stems.contains(&norm)
    }
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| *c != '.').collect::<String>().to_lowercase()
}

#[derive(Clone, Debug)]
pub struct Package {
    pub dir: PathBuf,
    pub version: String,
    pub components: Vec<Component>,
    /// Bytes of everything outside the components (installer, setup.cfg, licence).
    pub base_bytes: u64,
}

impl Package {
    pub fn total_bytes(&self) -> u64 {
        self.base_bytes + self.components.iter().map(|c| c.bytes).sum::<u64>()
    }

    pub fn kept_bytes(&self, keep: &BTreeSet<String>) -> u64 {
        self.base_bytes + self.components.iter().filter(|c| keep.contains(&c.folder)).map(|c| c.bytes).sum::<u64>()
    }

    fn by_folder(&self, folder: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.folder == folder)
    }

    /// The folder that provides a package name, if it's in this package.
    fn provider(&self, package: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.provides(package))
    }

    /// Folders that `folder` needs, directly or through others.
    pub fn needs(&self, folder: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut todo = vec![folder.to_string()];
        while let Some(f) = todo.pop() {
            let Some(c) = self.by_folder(&f) else { continue };
            for r in &c.requires {
                if let Some(p) = self.provider(r) {
                    if p.folder != folder && out.insert(p.folder.clone()) {
                        todo.push(p.folder.clone());
                    }
                }
            }
        }
        out
    }

    /// Folders that need `folder`, directly or through others.
    pub fn needed_by(&self, folder: &str) -> BTreeSet<String> {
        self.components.iter().filter(|c| c.folder != folder && self.needs(&c.folder).contains(folder)).map(|c| c.folder.clone()).collect()
    }

    /// The folders a preset keeps, with every requirement of a kept folder kept too.
    pub fn preset(&self, preset: Preset) -> BTreeSet<String> {
        let mut keep: BTreeSet<String> =
            self.components.iter().filter(|c| preset.keeps(c.role(), c.main())).map(|c| c.folder.clone()).collect();
        self.complete(&mut keep);
        keep
    }

    /// Adds whatever kept folders require, and the folders that may never be left out.
    pub fn complete(&self, keep: &mut BTreeSet<String>) {
        for c in &self.components {
            if Preset::Custom.locked(c.role()) {
                keep.insert(c.folder.clone());
            }
        }
        let kept: Vec<String> = keep.iter().cloned().collect();
        for f in kept {
            keep.extend(self.needs(&f));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Preset {
    #[default]
    Optimized,
    Competitive,
    Minimal,
    Custom,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Optimized, Preset::Competitive, Preset::Minimal, Preset::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Optimized => "Optimized Gaming",
            Preset::Competitive => "Competitive",
            Preset::Minimal => "Minimal",
            Preset::Custom => "Custom",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            Preset::Optimized => {
                "The driver with every gaming feature, without the NVIDIA App, its background services, overlay, recorder and telemetry."
            }
            Preset::Competitive => {
                "Optimized Gaming without the deep-learning scaling model, which only matters for DLDSR. The same background footprint: there is nothing left that runs."
            }
            Preset::Minimal => {
                "Only the driver and what your hardware needs. Also leaves out PhysX and sound over HDMI/DisplayPort."
            }
            Preset::Custom => "Choose each component yourself. Requirements are kept automatically.",
        }
    }

    fn keeps(self, role: Role, package: &str) -> bool {
        match (self, role) {
            (_, Role::Core | Role::Hardware | Role::Unknown) => true,
            (_, Role::App) => false,
            (Preset::Optimized | Preset::Custom, Role::Feature) => true,
            (Preset::Competitive, Role::Feature) => package != "NvDLISR",
            (Preset::Minimal, Role::Feature) => false,
        }
    }

    /// Components that can't be switched off.
    pub fn locked(self, role: Role) -> bool {
        matches!(role, Role::Core | Role::Hardware)
    }
}

pub(crate) fn dir_size(p: &Path) -> u64 {
    let Ok(md) = fs::symlink_metadata(p) else { return 0 };
    if md.is_file() {
        return md.len();
    }
    fs::read_dir(p).map_or(0, |rd| rd.flatten().map(|e| dir_size(&e.path())).sum())
}

/// Attribute value from an XML start tag's attribute text.
fn attr(tag: &str, name: &str) -> Option<String> {
    let i = tag.find(name)?;
    let rest = tag[i + name.len()..].trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

/// (package name, required package names) from one `.nvi` manifest. Template names (`${{…}}`) are skipped.
fn read_nvi(text: &str) -> (Option<String>, Vec<String>) {
    let name = text.find("<nvi").and_then(|i| {
        let end = text[i..].find('>')? + i;
        attr(&text[i + 4..end], " name").or_else(|| attr(&text[i + 4..end], "name"))
    });
    let mut requires = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("<package ") {
        let end = rest[i..].find('>').map_or(rest.len(), |e| e + i);
        let tag = &rest[i..end];
        if attr(tag, "type").as_deref() == Some("requires") {
            if let Some(p) = attr(tag, " package").filter(|p| !p.contains("${")) {
                requires.push(p);
            }
        }
        rest = &rest[end..];
    }
    (name.filter(|n| !n.contains("${")), requires)
}

fn nvi_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            nvi_files(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nvi")) {
            out.push(p);
        }
    }
}

/// Reads an unpacked package. Fails when it isn't an NVIDIA driver package Kestrel can handle.
pub fn analyze(dir: &Path) -> Result<Package, String> {
    let cfg = fs::read_to_string(dir.join("setup.cfg")).map_err(|_| "This isn't an NVIDIA driver package (no setup.cfg).".to_string())?;
    let version = cfg.find("<setup").and_then(|i| attr(&cfg[i..i + cfg[i..].find('>')?], "version")).unwrap_or_default();
    if !dir.join("Display.Driver").is_dir() || !dir.join("setup.exe").is_file() {
        return Err("This package has no graphics driver in it (Display.Driver is missing).".into());
    }
    let mut components = Vec::new();
    let mut base_bytes = 0;
    for e in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let path = e.path();
        let folder = e.file_name().to_string_lossy().into_owned();
        // The installer itself (NVI2) and loose files are always kept.
        if !path.is_dir() || folder.eq_ignore_ascii_case("NVI2") {
            base_bytes += dir_size(&path);
            continue;
        }
        let mut files = Vec::new();
        nvi_files(&path, &mut files);
        let (mut packages, mut requires, mut stems) = (Vec::new(), Vec::new(), Vec::new());
        for f in &files {
            let text = fs::read_to_string(f).unwrap_or_default();
            let (name, req) = read_nvi(text.trim_start_matches('\u{feff}'));
            packages.extend(name);
            requires.extend(req);
            stems.extend(f.file_stem().map(|s| norm(&s.to_string_lossy())));
        }
        // A folder without manifests (data for another package) stays with the package.
        if files.is_empty() {
            base_bytes += dir_size(&path);
            continue;
        }
        components.push(Component { folder, packages, stems, requires, bytes: dir_size(&path) });
    }
    components.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    Ok(Package { dir: dir.to_path_buf(), version, components, base_bytes })
}

/// setup.cfg with the `<manifest>` file entries that pointed into left-out folders removed (the installer refuses
/// to start when a listed file is missing). Everything else is unchanged.
pub fn patch_setup_cfg(cfg: &str, removed: &BTreeSet<String>) -> String {
    // Top-level (not localized) string values, for expanding ${{Name}}.
    let mut strings = BTreeMap::new();
    let mut rest = cfg;
    while let Some(i) = rest.find("<string name=\"") {
        let end = rest[i..].find('>').map_or(rest.len(), |e| e + i);
        let tag = &rest[i..end];
        if let (Some(n), Some(v)) = (attr(tag, "name"), attr(tag, "value")) {
            strings.entry(n).or_insert(v);
        }
        rest = &rest[end..];
    }
    let expand = |s: &str| {
        let mut s = s.to_string();
        for _ in 0..5 {
            let Some(a) = s.find("${{") else { break };
            let Some(b) = s[a..].find("}}") else { break };
            let key = s[a + 3..a + b].to_string();
            s.replace_range(a..a + b + 2, strings.get(&key).map_or("", String::as_str));
        }
        s
    };
    let manifest = cfg.find("<manifest>").zip(cfg.find("</manifest>"));
    cfg.split_inclusive('\n')
        .scan(0usize, |pos, line| {
            let start = *pos;
            *pos += line.len();
            Some((start, line))
        })
        .filter(|(start, line)| {
            let in_manifest = manifest.is_some_and(|(a, b)| *start > a && *start < b);
            let Some(file) = line.trim().strip_prefix("<file ").and_then(|t| attr(t, "name")) else { return true };
            if !in_manifest {
                return true;
            }
            let path = expand(&file);
            let first = path.split(['\\', '/']).next().unwrap_or("");
            !removed.iter().any(|r| r.eq_ignore_ascii_case(first))
        })
        .map(|(_, l)| l)
        .collect()
}

/// Copies one file or folder, hard-linking files when possible (same drive: no extra space, nothing re-written).
fn link_tree(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|e| format!("Couldn't create {}: {e}", to.display()))?;
        for e in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
            link_tree(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        fs::hard_link(from, to).or_else(|_| fs::copy(from, to).map(|_| ())).map_err(|e| format!("Couldn't copy {}: {e}", from.display()))
    }
}

/// Builds the stripped package in `out` (replaced if it exists): the kept folders, the installer, and a patched
/// setup.cfg. Returns the folders left out.
pub fn build(pkg: &Package, keep: &BTreeSet<String>, out: &Path) -> Result<BTreeSet<String>, String> {
    let removed: BTreeSet<String> = pkg.components.iter().filter(|c| !keep.contains(&c.folder)).map(|c| c.folder.clone()).collect();
    for c in &pkg.components {
        if keep.contains(&c.folder) {
            let missing: Vec<String> = pkg.needs(&c.folder).into_iter().filter(|n| removed.contains(n)).collect();
            if !missing.is_empty() {
                return Err(format!("{} needs {}, which is left out.", c.folder, missing.join(", ")));
            }
        }
    }
    if pkg.by_folder("Display.Driver").is_none_or(|_| !keep.contains("Display.Driver")) {
        return Err("The graphics driver can't be left out.".into());
    }
    if out.exists() {
        fs::remove_dir_all(out).map_err(|e| format!("Couldn't clear the old build: {e}"))?;
    }
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for e in fs::read_dir(&pkg.dir).map_err(|e| e.to_string())?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if removed.contains(&name) || name.eq_ignore_ascii_case("setup.cfg") {
            continue;
        }
        link_tree(&e.path(), &out.join(&name))?;
    }
    let cfg = fs::read_to_string(pkg.dir.join("setup.cfg")).map_err(|e| e.to_string())?;
    fs::write(out.join("setup.cfg"), patch_setup_cfg(&cfg, &removed)).map_err(|e| e.to_string())?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comp(folder: &str, packages: &[&str], requires: &[&str]) -> Component {
        Component {
            folder: folder.into(),
            packages: packages.iter().map(|s| s.to_string()).collect(),
            stems: packages.iter().map(|s| norm(s)).collect(),
            requires: requires.iter().map(|s| s.to_string()).collect(),
            bytes: 10,
        }
    }

    fn sample() -> Package {
        Package {
            dir: PathBuf::new(),
            version: "621.40".into(),
            base_bytes: 5,
            components: vec![
                comp("Display.Driver", &["Display.Driver"], &[]),
                comp("NvApp", &["Display.NvApp"], &["NvTelemetry", "ShadowPlay"]),
                comp("NvTelemetry", &["NvTelemetry"], &["NvContainer.LocalSystem"]),
                // As in the real package: one named manifest, and NvContainerLocalSystem.nvi with a template name.
                Component {
                    stems: vec!["nvcontaineruser".into(), "nvcontainerlocalsystem".into()],
                    ..comp("NvContainer", &["NvContainer.User"], &[])
                },
                comp("ShadowPlay", &["ShadowPlay"], &["VirtualAudio.Driver"]),
                comp("NvVAD", &["VirtualAudio.Driver"], &[]),
                comp("PhysX", &["Display.PhysX"], &[]),
                comp("HDAudio", &["HDAudio.Driver"], &[]),
                comp("NvDLISR", &["NvDLISR"], &[]),
                comp("PPC", &["USBC"], &["Display.Driver"]),
                comp("Future", &["Display.Something"], &[]),
            ],
        }
    }

    #[test]
    fn presets_keep_what_gaming_needs() {
        let p = sample();
        let opt = p.preset(Preset::Optimized);
        for f in ["Display.Driver", "PhysX", "HDAudio", "NvDLISR", "PPC", "Future"] {
            assert!(opt.contains(f), "{f}");
        }
        for f in ["NvApp", "NvTelemetry", "NvContainer", "ShadowPlay", "NvVAD"] {
            assert!(!opt.contains(f), "{f}");
        }
        assert!(!p.preset(Preset::Competitive).contains("NvDLISR"));
        let min = p.preset(Preset::Minimal);
        assert!(!min.contains("PhysX") && !min.contains("HDAudio") && min.contains("PPC") && min.contains("Future"));
    }

    #[test]
    fn requirements_follow_the_manifests() {
        let p = sample();
        // Template-named packages resolve through the manifest file name (NvContainerLocalSystem.nvi).
        assert_eq!(p.needs("NvApp"), ["NvContainer", "NvTelemetry", "NvVAD", "ShadowPlay"].map(String::from).into());
        assert!(p.needed_by("NvVAD").contains("NvApp"));
        let mut keep: BTreeSet<String> = ["Display.Driver", "ShadowPlay"].map(String::from).into();
        p.complete(&mut keep);
        assert!(keep.contains("NvVAD") && keep.contains("PPC") && !keep.contains("NvApp"));
    }

    #[test]
    fn manifests_are_read() {
        let nvi = r#"<?xml version="1.0"?><nvi
  name      = "ShadowPlay"
  version="1"><dependencies><package type="requires" package="VirtualAudio.Driver"/><package type="after" package="Display.Driver"/>
  <package type="requires" package="${{RequiredPackage}}"/></dependencies></nvi>"#;
        assert_eq!(read_nvi(nvi), (Some("ShadowPlay".into()), vec!["VirtualAudio.Driver".into()]));
        assert_eq!(read_nvi(r#"<nvi name="${{PackageId}}" >"#).0, None);
    }

    #[test]
    fn setup_cfg_loses_only_removed_files() {
        let cfg = "<setup version=\"621.40\">\n\t<strings>\n\t\t<string name=\"EulaHtmlFile\" value=\"NvApp\\Unified_EULA\\EULA_${{LocaleName}}.html\"/>\n\t</strings>\n\t<strings><localized><string name=\"LocaleName\" value=\"en-US\"/></localized></strings>\n\t<install>\n\t\t<file name=\"${{EulaHtmlFile}}\"/>\n\t</install>\n\t<manifest>\n\t\t<file name=\"setup.exe\" core=\"true\"/>\n\t\t<file name=\"${{EulaHtmlFile}}\"/>\n\t\t<file name=\"eula.txt\"/>\n\t</manifest>\n</setup>\n";
        let removed: BTreeSet<String> = ["NvApp".to_string()].into();
        let out = patch_setup_cfg(cfg, &removed);
        assert!(out.contains("<file name=\"setup.exe\" core=\"true\"/>") && out.contains("<file name=\"eula.txt\"/>"));
        assert_eq!(out.matches("${{EulaHtmlFile}}").count(), 1, "only the manifest entry goes");
        assert_eq!(patch_setup_cfg(cfg, &BTreeSet::new()), cfg);
    }

    /// KESTREL_NV_PKG=<unpacked package folder>: analyze it, print the presets and build Optimized next to it.
    #[test]
    #[ignore = "needs an unpacked NVIDIA package"]
    fn real_package() {
        let Ok(dir) = std::env::var("KESTREL_NV_PKG") else { return };
        let p = analyze(Path::new(&dir)).unwrap();
        println!("version {} total {} MB", p.version, p.total_bytes() >> 20);
        for c in &p.components {
            println!("{:18} {:5} MB {:?} {:?} needs {:?}", c.folder, c.bytes >> 20, c.role(), c.packages, p.needs(&c.folder));
        }
        for preset in Preset::ALL {
            let keep = p.preset(preset);
            println!("{:16} keeps {:?} = {} MB", preset.label(), keep, p.kept_bytes(&keep) >> 20);
        }
        let out = PathBuf::from(format!("{dir}-optimized"));
        let removed = build(&p, &p.preset(Preset::Optimized), &out).unwrap();
        println!("removed {removed:?}");
        let cfg = fs::read_to_string(out.join("setup.cfg")).unwrap();
        assert!(!cfg.contains("<file name=\"${{EulaHtmlFile}}\"/>"));
        assert!(out.join("Display.Driver").join("nv_dispi.inf").is_file());
        // The hard-linked build deletes cleanly and leaves the unpacked package whole.
        fs::remove_dir_all(&out).unwrap();
        assert!(Path::new(&dir).join("Display.Driver").join("nv_dispi.inf").is_file());
    }
}
