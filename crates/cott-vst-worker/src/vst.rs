use anyhow::{Context, Result, anyhow};
use cott_ipc::{
    ParamInfo, PluginDescriptor, PluginFormat, TransportInfo, posix::SharedAudioRegion,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use truce_rack_clap::ClapScanner;
use truce_rack_core::events::{Event, EventBody, EventList, MidiData};
use truce_rack_core::info::PluginCategory;
use truce_rack_core::scanner::PluginScanner;
use truce_rack_core::transport::TransportInfo as RackTransport;
use truce_rack_core::{AudioBuffer as RackBuffer, BusLayout};
use truce_rack_lv2::Lv2Scanner;
use truce_rack_vst3::Vst3Scanner;

use crate::x11_editor::FloatingEditorWindow;

type LoadedVst = <Vst3Scanner as PluginScanner>::Plugin;
type LoadedClap = <ClapScanner as PluginScanner>::Plugin;
type LoadedLv2 = <Lv2Scanner as PluginScanner>::Plugin;

enum RackPlugin {
    Vst3(Box<LoadedVst>),
    Clap(Box<LoadedClap>),
    Lv2(Box<LoadedLv2>),
}

impl RackPlugin {
    fn plugin(&self) -> &dyn truce_rack_core::plugin::Plugin<f32> {
        match self {
            Self::Vst3(plugin) => plugin.as_ref(),
            Self::Clap(plugin) => plugin.as_ref(),
            Self::Lv2(plugin) => plugin.as_ref(),
        }
    }

    fn plugin_mut(&mut self) -> &mut dyn truce_rack_core::plugin::Plugin<f32> {
        match self {
            Self::Vst3(plugin) => plugin.as_mut(),
            Self::Clap(plugin) => plugin.as_mut(),
            Self::Lv2(plugin) => plugin.as_mut(),
        }
    }

    fn pump_host_services(&mut self) {
        if let Self::Vst3(plugin) = self {
            plugin.pump_host_services();
        }
    }

    fn latency_samples(&self) -> u32 {
        match self {
            Self::Vst3(plugin) => plugin.latency_samples(),
            Self::Clap(_) | Self::Lv2(_) => 0,
        }
    }
}

pub struct VstPlugin {
    plugin: RackPlugin,
    sample_rate: f64,
    block_size: usize,
    is_instrument: bool,
    has_editor: bool,
    name: String,
    params: Vec<ParamInfo>,
    latency: u32,
    /// Owned floating X11 parent when the host did not supply one.
    owned_editor: Option<FloatingEditorWindow>,
}

pub enum PluginBackend {
    Rack(Box<VstPlugin>),
    Vst2(Box<crate::vst2::Vst2Plugin>),
}

impl PluginBackend {
    pub fn load(
        format: PluginFormat,
        path: &Path,
        uid: &str,
        sample_rate: f64,
        block_size: u32,
        state: Option<&[u8]>,
    ) -> Result<Self> {
        match format {
            PluginFormat::Vst2 => Ok(Self::Vst2(Box::new(crate::vst2::Vst2Plugin::load(
                path,
                sample_rate,
                block_size,
                state,
            )?))),
            _ => Ok(Self::Rack(Box::new(VstPlugin::load(
                format,
                path,
                uid,
                sample_rate,
                block_size,
                state,
            )?))),
        }
    }

    pub fn meta(&mut self) -> (String, u32, Vec<ParamInfo>, bool, bool) {
        match self {
            Self::Rack(plugin) => plugin.meta(),
            Self::Vst2(plugin) => plugin.descriptor_meta(),
        }
    }

    pub fn params(&self) -> Vec<ParamInfo> {
        match self {
            Self::Rack(plugin) => plugin.params(),
            Self::Vst2(plugin) => plugin.params(),
        }
    }

    pub fn set_param(&mut self, id: u32, value: f32, normalized: bool) {
        match self {
            Self::Rack(plugin) => plugin.set_param(id, value, normalized),
            Self::Vst2(plugin) => plugin.set_param(id, value),
        }
    }

    pub fn get_state(&self) -> Vec<u8> {
        match self {
            Self::Rack(plugin) => plugin.get_state(),
            Self::Vst2(plugin) => plugin.get_state(),
        }
    }

    pub fn set_state(&mut self, data: &[u8]) {
        match self {
            Self::Rack(plugin) => plugin.set_state(data),
            Self::Vst2(plugin) => plugin.set_state(data),
        }
    }

    pub fn latency(&self) -> u32 {
        match self {
            Self::Rack(plugin) => plugin.latency(),
            Self::Vst2(plugin) => plugin.latency(),
        }
    }

    pub fn open_editor(&mut self, parent_x11: Option<u64>) -> Result<()> {
        match self {
            Self::Rack(plugin) => plugin.open_editor(parent_x11),
            Self::Vst2(plugin) => plugin.open_editor(parent_x11),
        }
    }

    pub fn close_editor(&mut self) {
        match self {
            Self::Rack(plugin) => plugin.close_editor(),
            Self::Vst2(plugin) => plugin.close_editor(),
        }
    }

    pub fn pump_editor(&mut self) -> bool {
        match self {
            Self::Rack(plugin) => plugin.pump_editor(),
            Self::Vst2(plugin) => plugin.pump_editor(),
        }
    }

    pub fn process(&mut self, shm: &mut SharedAudioRegion, transport: &TransportInfo) -> bool {
        match self {
            Self::Rack(plugin) => plugin.process(shm, transport),
            Self::Vst2(plugin) => plugin.process(shm, transport),
        }
    }
}

pub fn scan_paths(paths: &[PathBuf]) -> Result<Vec<PluginDescriptor>> {
    let sources: Vec<PathBuf> = if paths.is_empty() {
        standard_vst3_dirs()
    } else {
        paths.to_vec()
    };
    let bundles = discover_vst3_bundles(&sources);
    let mut out: Vec<PluginDescriptor> = Vec::new();

    for bundle in bundles {
        // Yabridge: never ModuleEntry during catalog scan — each one starts Wine
        // and can hang / spam desktop notifications. List from the filesystem;
        // real factory IDs are resolved on load.
        if looks_like_yabridge(&bundle) {
            let desc = descriptor_from_bundle_path(
                &bundle,
                PluginFormat::Vst3,
                /*prefer_instrument*/ true,
            );
            if !out.iter().any(|d| d.path == desc.path) {
                out.push(desc);
            }
            continue;
        }

        match scan_one_bundle(&bundle) {
            Ok((_tmp, list)) => {
                for info in list {
                    let desc = info_to_desc_no_reprobe(&info);
                    if !out.iter().any(|d| d.uid == desc.uid || d.path == desc.path) {
                        out.push(desc);
                    }
                }
            }
            Err(e) => {
                warn!(
                    "native scan {}: {e:#} — listing by path only",
                    bundle.display()
                );
                let desc = descriptor_from_bundle_path(
                    &bundle,
                    PluginFormat::Vst3,
                    /*prefer_instrument*/ false,
                );
                if !out.iter().any(|d| d.path == desc.path) {
                    out.push(desc);
                }
            }
        }
    }

    scan_clap_plugins(&mut out);
    scan_lv2_plugins(&mut out);
    out.extend(crate::vst2::scan_paths(&standard_vst2_dirs()));
    deduplicate_catalog(&mut out);
    info!("catalogued {} plugins across VST2/VST3/CLAP/LV2", out.len());
    Ok(out)
}

fn scan_clap_plugins(out: &mut Vec<PluginDescriptor>) {
    let scanner = ClapScanner::new();
    for dir in standard_clap_dirs() {
        let mut scan_dirs = Vec::new();
        collect_non_bundle_dirs(&dir, ".clap", &mut scan_dirs);
        for scan_dir in scan_dirs {
            let Ok(infos) = scanner.scan_path(&scan_dir) else {
                continue;
            };
            for info in infos {
                let desc = info_to_desc(&info, PluginFormat::Clap);
                if !out
                    .iter()
                    .any(|existing| existing.format == desc.format && existing.uid == desc.uid)
                {
                    out.push(desc);
                }
            }
        }
        // Defer yabridge CLAP factory startup until the user loads it.
        collect_deferred_clap_wrappers(&dir, out);
    }
}

fn scan_lv2_plugins(out: &mut Vec<PluginDescriptor>) {
    match Lv2Scanner::new().scan() {
        Ok(infos) => out.extend(
            infos
                .iter()
                .map(|info| info_to_desc(info, PluginFormat::Lv2)),
        ),
        Err(err) => warn!("LV2 scan failed: {err}"),
    }
}

fn collect_non_bundle_dirs(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    if !dir.is_dir()
        || dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(extension))
    {
        return;
    }
    if !looks_like_yabridge_path(dir) {
        out.push(dir.to_owned());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            collect_non_bundle_dirs(&entry.path(), extension, out);
        }
    }
}

fn collect_deferred_clap_wrappers(dir: &Path, out: &mut Vec<PluginDescriptor>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_deferred_clap_wrappers(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("clap")
            && looks_like_yabridge_path(&path)
        {
            let desc = descriptor_from_bundle_path(&path, PluginFormat::Clap, true);
            if !out.iter().any(|existing| existing.path == desc.path) {
                out.push(desc);
            }
        }
    }
}

fn deduplicate_catalog(catalog: &mut Vec<PluginDescriptor>) {
    let mut seen = std::collections::HashSet::new();
    catalog.retain(|plugin| seen.insert((plugin.format, plugin.uid.clone(), plugin.path.clone())));
}

fn standard_vst3_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/lib/vst3"),
        PathBuf::from("/usr/local/lib/vst3"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".vst3"));
    }
    if let Some(extra) = std::env::var_os("VST3_PATH") {
        dirs.extend(std::env::split_paths(&extra));
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn standard_vst2_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/lib/vst"),
        PathBuf::from("/usr/local/lib/vst"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".vst"));
    }
    if let Some(extra) = std::env::var_os("VST_PATH") {
        dirs.extend(std::env::split_paths(&extra));
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn standard_clap_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/lib/clap"),
        PathBuf::from("/usr/local/lib/clap"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".clap"));
    }
    if let Some(extra) = std::env::var_os("CLAP_PATH") {
        dirs.extend(std::env::split_paths(&extra));
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Collect every directory that may contain `.vst3` bundles, including vendor
/// subfolders. Bundle directories themselves (names ending in `.vst3`) are leaves.
fn expand_vst3_scan_dirs(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for root in roots {
        collect_vst3_scan_dirs(root, &mut dirs);
    }
    dirs
}

fn collect_vst3_scan_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() || is_vst3_bundle(dir) {
        return;
    }
    if !out.iter().any(|d| d == dir) {
        out.push(dir.to_path_buf());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && !is_vst3_bundle(&path) {
            collect_vst3_scan_dirs(&path, out);
        }
    }
}

fn discover_vst3_bundles(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut bundles = Vec::new();
    for dir in expand_vst3_scan_dirs(roots) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if is_vst3_bundle(&path) {
                bundles.push(path);
            }
        }
    }
    bundles.sort();
    bundles.dedup();
    bundles
}

fn is_vst3_bundle(path: &Path) -> bool {
    // The user install root is literally named `.vst3` — that is a folder of
    // plugins, not a plugin bundle. Real bundles look like `Foo.vst3` and
    // contain a `Contents` directory.
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name == ".vst3" || !name.ends_with(".vst3") {
        return false;
    }
    path.join("Contents").is_dir()
}

fn looks_like_yabridge(bundle: &Path) -> bool {
    looks_like_yabridge_path(bundle)
}

pub(crate) fn looks_like_yabridge_path(bundle: &Path) -> bool {
    bundle.components().any(|c| c.as_os_str() == "yabridge")
        || bundle.join("Contents").join("x86_64-win").is_dir()
        || bundle.join("Contents").join("x86-win").is_dir()
}

pub(crate) fn stable_path_hash(path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn descriptor_from_bundle_path(
    bundle: &Path,
    format: PluginFormat,
    prefer_instrument: bool,
) -> PluginDescriptor {
    let name = bundle
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Plugin")
        .to_string();
    let is_instrument = if crate::classify::name_looks_like_effect(&name) {
        false
    } else if crate::classify::name_looks_like_instrument(&name) {
        true
    } else {
        prefer_instrument
    };
    let ambiguous = !crate::classify::name_looks_like_instrument(&name)
        && !crate::classify::name_looks_like_effect(&name);
    PluginDescriptor {
        format,
        uid: format!("{}-path:{}", format.as_str(), stable_path_hash(bundle)),
        name,
        vendor: if looks_like_yabridge(bundle) {
            "yabridge".into()
        } else {
            String::new()
        },
        path: bundle.to_path_buf(),
        is_instrument,
        // Deferred yabridge entries may not expose category metadata until Wine
        // starts. Ambiguous names are offered in both browser sections.
        is_effect: !is_instrument || ambiguous,
        has_editor: true,
    }
}

/// Scan a single `.vst3` via a temp dir symlink so sibling plugins are not opened.
/// Keeps the temp dir alive for the caller until `load` finishes (symlink target).
fn scan_one_bundle(
    bundle: &Path,
) -> Result<(tempfile::TempDir, Vec<truce_rack_core::info::PluginInfo>)> {
    let scanner = Vst3Scanner::new();
    let tmp = tempfile::tempdir().context("temp dir for isolated VST scan")?;
    let link = tmp.path().join(
        bundle
            .file_name()
            .ok_or_else(|| anyhow!("bundle has no file name"))?,
    );
    std::os::unix::fs::symlink(bundle, &link).context("symlink bundle for isolated scan")?;
    let mut list = scanner
        .scan_path(tmp.path())
        .map_err(|e| anyhow!("scan_path: {e}"))?;
    // Prefer the real bundle path for subsequent load/dlopen.
    for info in &mut list {
        info.path = bundle.to_path_buf();
    }
    Ok((tmp, list))
}

fn info_to_desc_no_reprobe(info: &truce_rack_core::info::PluginInfo) -> PluginDescriptor {
    info_to_desc(info, PluginFormat::Vst3)
}

fn info_to_desc(
    info: &truce_rack_core::info::PluginInfo,
    format: PluginFormat,
) -> PluginDescriptor {
    // Do not re-open the module here (that re-triggers yabridge/Wine).
    let is_instrument = matches!(info.category, PluginCategory::Instrument)
        || info.accepts_midi
        || crate::classify::name_looks_like_instrument(&info.name);
    PluginDescriptor {
        format,
        uid: info.unique_id.clone(),
        name: info.name.clone(),
        vendor: info.vendor.clone(),
        path: info.path.clone(),
        is_instrument,
        is_effect: !is_instrument,
        has_editor: info.has_editor,
    }
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn pick_plugin_info(
    infos: Vec<truce_rack_core::info::PluginInfo>,
    uid: &str,
    path: &Path,
) -> Result<truce_rack_core::info::PluginInfo> {
    if let Some(info) = infos
        .iter()
        .find(|i| i.unique_id == uid || paths_equal(&i.path, path))
        .cloned()
    {
        return Ok(info);
    }
    // Catalog may have used a path-hash uid; take the sole Audio Module if unique.
    if infos.len() == 1 {
        return Ok(infos.into_iter().next().unwrap());
    }
    Err(anyhow!(
        "plugin uid {uid} not found (path {}, candidates={})",
        path.display(),
        infos.len()
    ))
}

fn scan_one_clap(
    plugin_path: &Path,
) -> Result<(tempfile::TempDir, Vec<truce_rack_core::info::PluginInfo>)> {
    let scanner = ClapScanner::new();
    let tmp = tempfile::tempdir().context("temp dir for isolated CLAP scan")?;
    let link = tmp.path().join(
        plugin_path
            .file_name()
            .ok_or_else(|| anyhow!("CLAP path has no file name"))?,
    );
    std::os::unix::fs::symlink(plugin_path, &link).context("symlink CLAP for isolated scan")?;
    let mut infos = scanner
        .scan_path(tmp.path())
        .map_err(|err| anyhow!("scan CLAP: {err}"))?;
    for info in &mut infos {
        info.path = plugin_path.to_owned();
    }
    Ok((tmp, infos))
}

impl VstPlugin {
    pub fn load(
        format: PluginFormat,
        path: &Path,
        uid: &str,
        sample_rate: f64,
        block_size: u32,
        state: Option<&[u8]>,
    ) -> Result<Self> {
        let (info, mut plugin) = match format {
            PluginFormat::Vst3 => {
                let scanner = Vst3Scanner::new();
                // Only open THIS bundle — never scan a whole yabridge directory.
                let (_tmp_guard, infos) = if path.exists() && is_vst3_bundle(path) {
                    scan_one_bundle(path).context("load: isolated VST3 scan")?
                } else if path.exists() {
                    let parent = path.parent().unwrap_or(path);
                    let list = scanner
                        .scan_path(parent)
                        .map_err(|e| anyhow!("scan VST3 parent: {e}"))?;
                    (tempfile::tempdir().context("dummy temp")?, list)
                } else {
                    return Err(anyhow!("plugin path missing: {}", path.display()));
                };
                let info = pick_plugin_info(infos, uid, path)?;
                let plugin = scanner.load(&info).context("load VST3")?;
                (info, RackPlugin::Vst3(Box::new(plugin)))
            }
            PluginFormat::Clap => {
                if !path.exists() {
                    return Err(anyhow!("plugin path missing: {}", path.display()));
                }
                let scanner = ClapScanner::new();
                let (_tmp_guard, infos) =
                    scan_one_clap(path).context("load: isolated CLAP scan")?;
                let info = pick_plugin_info(infos, uid, path)?;
                let plugin = scanner.load(&info).context("load CLAP")?;
                (info, RackPlugin::Clap(Box::new(plugin)))
            }
            PluginFormat::Lv2 => {
                let scanner = Lv2Scanner::new();
                let infos = scanner.scan().context("scan LV2 world")?;
                let info = pick_plugin_info(infos, uid, path)?;
                let plugin = scanner.load(&info).context("load LV2")?;
                (info, RackPlugin::Lv2(Box::new(plugin)))
            }
            PluginFormat::Vst2 => return Err(anyhow!("VST2 uses the legacy backend")),
        };
        let layout = plugin
            .plugin()
            .supported_layouts()
            .first()
            .cloned()
            .unwrap_or_else(BusLayout::stereo);
        plugin
            .plugin_mut()
            .activate(layout, sample_rate, block_size as usize)
            .context("activate")?;
        if let Some(bytes) = state
            && !bytes.is_empty()
        {
            let _ = plugin.plugin_mut().load_state(bytes);
        }
        let mut params = Vec::new();
        for i in 0..plugin.plugin().parameter_count() {
            if let Ok(p) = plugin.plugin().parameter_info(i) {
                params.push(ParamInfo {
                    id: p.id,
                    name: p.name,
                    default: p.default as f32,
                    min: p.min as f32,
                    max: p.max as f32,
                });
            }
        }
        let latency = plugin.latency_samples();
        let name = info.name.clone();
        // Prefer VST category / MIDI IO — do not force yabridge shells to instruments
        // (yabridge FX would otherwise take the wrong MIDI/audio path).
        let is_instrument = matches!(info.category, PluginCategory::Instrument)
            || info.accepts_midi
            || crate::classify::name_looks_like_instrument(&info.name);
        let has_editor = info.has_editor;
        info!(
            "loaded {format} {name} instrument={is_instrument} editor={has_editor} latency={latency}"
        );
        Ok(Self {
            plugin,
            sample_rate,
            block_size: block_size as usize,
            is_instrument,
            has_editor,
            name,
            params,
            latency,
            owned_editor: None,
        })
    }

    pub fn meta(&self) -> (String, u32, Vec<ParamInfo>, bool, bool) {
        (
            self.name.clone(),
            self.latency,
            self.params.clone(),
            self.has_editor,
            self.is_instrument,
        )
    }

    pub fn params(&self) -> Vec<ParamInfo> {
        self.params.clone()
    }

    pub fn set_param(&mut self, id: u32, value: f32, normalized: bool) {
        if let Some(idx) = self.params.iter().position(|p| p.id == id) {
            let parameter = &self.params[idx];
            let value = if normalized {
                parameter.min + value.clamp(0.0, 1.0) * (parameter.max - parameter.min)
            } else {
                value.clamp(parameter.min, parameter.max)
            };
            let _ = self.plugin.plugin_mut().set_parameter(idx, value as f64);
        }
    }

    pub fn get_state(&self) -> Vec<u8> {
        self.plugin.plugin().save_state().unwrap_or_default()
    }

    pub fn set_state(&mut self, data: &[u8]) {
        let _ = self.plugin.plugin_mut().load_state(data);
    }

    pub fn latency(&self) -> u32 {
        self.latency
    }

    pub fn refresh_latency(&mut self) {
        self.latency = self.plugin.latency_samples();
    }

    pub fn open_editor(&mut self, parent_x11: Option<u64>) -> Result<()> {
        // Close any previous owned floating window first.
        self.close_editor();

        let (parent_id, owned) = match parent_x11 {
            Some(id) => (id, None),
            None => {
                info!(
                    plugin = %self.name,
                    "no host X11 parent — creating floating editor window"
                );
                let win = FloatingEditorWindow::create_default(&self.name)
                    .context("create floating X11 editor parent")?;
                let id = win.embed_window_id();
                (id, Some(win))
            }
        };

        let handle = truce_rack_core::editor::WindowHandle::X11(parent_id);
        let preferred_size = {
            let editor = self
                .plugin
                .plugin_mut()
                .editor()
                .ok_or_else(|| anyhow!("plugin has no editor"))?;
            editor
                .open(handle, 1.0)
                .map_err(|e| anyhow!("open editor: {e}"))?;
            editor.size()
        };

        if let Some(mut win) = owned {
            if let Some((w, h)) = preferred_size {
                win.resize(w, h);
            }
            self.owned_editor = Some(win);
        }

        info!(plugin = %self.name, parent_id, "plugin editor opened");
        Ok(())
    }

    pub fn close_editor(&mut self) {
        if let Some(editor) = self.plugin.plugin_mut().editor() {
            editor.close();
        }
        self.owned_editor = None;
    }

    /// Pump X11 events for an owned floating editor. Returns `false` if the
    /// user closed the window (editor was closed as a side effect).
    pub fn pump_editor(&mut self) -> bool {
        // Linux VST3 UI timers / FD handlers.
        self.plugin.pump_host_services();

        let closed = match self.owned_editor.as_mut() {
            Some(win) => !win.pump_events(),
            None => {
                // Even without our floating window, keep the run-loop alive
                // so plugins that opened elsewhere still get timers.
                return true;
            }
        };
        if closed {
            if let Some(editor) = self.plugin.plugin_mut().editor() {
                editor.close();
            }
            self.owned_editor = None;
            return false;
        }
        if let Some(editor) = self.plugin.plugin_mut().editor() {
            editor.on_idle();
        }
        true
    }

    pub fn process(&mut self, shm: &mut SharedAudioRegion, transport: &TransportInfo) -> bool {
        let frames = transport.block_size as usize;
        if frames == 0 || frames > cott_ipc::MAX_BLOCK_FRAMES {
            return false;
        }

        let midi_count = shm
            .header()
            .midi_count
            .min(cott_ipc::MAX_MIDI_EVENTS as u32) as usize;
        let mut events = EventList::new();
        // VST3 IEventList has no ControlChange — truce-rack drops CC. Expand
        // All Notes/Sound Off into NoteOff storms so real plugins release.
        let mut panicked_channels = [false; 16];
        for ev in &shm.midi_mut()[..midi_count] {
            let channel = ev.status & 0x0f;
            let status = ev.status & 0xf0;
            if status == 0xb0 && (ev.data1 == 120 || ev.data1 == 123) {
                let ch = (channel as usize).min(15);
                if panicked_channels[ch] {
                    continue;
                }
                panicked_channels[ch] = true;
                for note in 0u8..=127 {
                    events.push(Event {
                        sample_offset: ev.sample_offset,
                        body: EventBody::Midi(MidiData::NoteOff {
                            channel,
                            note,
                            velocity: 0,
                        }),
                    });
                }
                continue;
            }
            let body = match status {
                0x90 if ev.data2 > 0 => EventBody::Midi(MidiData::NoteOn {
                    channel,
                    note: ev.data1,
                    velocity: ev.data2,
                }),
                0x80 | 0x90 => EventBody::Midi(MidiData::NoteOff {
                    channel,
                    note: ev.data1,
                    velocity: ev.data2,
                }),
                0xb0 => EventBody::Midi(MidiData::ControlChange {
                    channel,
                    controller: ev.data1,
                    value: ev.data2,
                }),
                _ => continue,
            };
            events.push(Event {
                sample_offset: ev.sample_offset,
                body,
            });
        }

        let mut in_l = vec![0.0f32; frames];
        let mut in_r = vec![0.0f32; frames];
        let mut out_l = vec![0.0f32; frames];
        let mut out_r = vec![0.0f32; frames];
        {
            let ain = shm.audio_in_mut();
            in_l.copy_from_slice(&ain[..frames]);
            in_r.copy_from_slice(
                &ain[cott_ipc::MAX_BLOCK_FRAMES..cott_ipc::MAX_BLOCK_FRAMES + frames],
            );
        }

        let input_refs: [&[f32]; 2] = [&in_l, &in_r];
        let mut output_refs: [&mut [f32]; 2] = [&mut out_l, &mut out_r];
        let bus_in = [truce_rack_core::buffer::BusRange::new(0, 2)];
        let bus_out = [truce_rack_core::buffer::BusRange::new(0, 2)];
        let mut buffer = RackBuffer::new(&input_refs, &mut output_refs, frames, &bus_in, &bus_out);

        let song_position_beats =
            transport.project_time_samples as f64 / transport.sample_rate * transport.tempo / 60.0;
        let beats_per_bar = transport.time_sig_numerator as f64 * 4.0
            / transport.time_sig_denominator.max(1) as f64;
        let rack_transport = RackTransport {
            tempo_bpm: Some(transport.tempo),
            time_signature: Some((transport.time_sig_numerator, transport.time_sig_denominator)),
            song_position_beats: Some(song_position_beats),
            song_position_samples: Some(transport.project_time_samples),
            bar_start_beats: Some(
                (song_position_beats / beats_per_bar.max(1.0)).floor() * beats_per_bar,
            ),
            playing: transport.playing,
            loop_active: transport.cycle,
            ..Default::default()
        };

        let mut output_events = EventList::new();
        let mut ctx = truce_rack_core::plugin::ProcessContext {
            sample_rate: self.sample_rate,
            max_block_size: self.block_size,
            transport: Some(rack_transport),
            output_events: &mut output_events,
        };

        let ok = match self
            .plugin
            .plugin_mut()
            .process(&mut buffer, &events, &mut ctx)
        {
            Ok(status) => !matches!(status, truce_rack_core::plugin::ProcessStatus::Error),
            Err(_) => false,
        };
        self.refresh_latency();

        {
            let aout = shm.audio_out_mut();
            aout[..frames].copy_from_slice(&out_l);
            aout[cott_ipc::MAX_BLOCK_FRAMES..cott_ipc::MAX_BLOCK_FRAMES + frames]
                .copy_from_slice(&out_r);
        }
        let header = shm.header_mut();
        header.frames = frames as u32;
        header.channels_out = 2;
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn expands_vendor_subfolders_but_skips_bundles() {
        let root = std::env::temp_dir().join(format!("cott-vst-scan-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let vendor = root.join("Vendor");
        let nested = vendor.join("Nested");
        let bundle = nested.join("CoolSynth.vst3");
        fs::create_dir_all(bundle.join("Contents")).unwrap();

        let dirs = expand_vst3_scan_dirs(std::slice::from_ref(&root));
        assert!(dirs.iter().any(|d| d == &root));
        assert!(dirs.iter().any(|d| d == &vendor));
        assert!(dirs.iter().any(|d| d == &nested));
        assert!(!dirs.iter().any(|d| d == &bundle));
        assert!(!dirs.iter().any(|d| d.ends_with("Contents")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn does_not_treat_dot_vst3_root_as_bundle() {
        let root =
            std::env::temp_dir().join(format!("cott-vst-root-test-{}/.vst3", std::process::id()));
        let _ = fs::remove_dir_all(root.parent().unwrap());
        let yabridge = root.join("yabridge");
        let bundle = yabridge.join("Pluto.vst3");
        fs::create_dir_all(bundle.join("Contents")).unwrap();

        assert!(!is_vst3_bundle(&root));
        assert!(is_vst3_bundle(&bundle));

        let dirs = expand_vst3_scan_dirs(std::slice::from_ref(&root));
        assert!(
            dirs.iter().any(|d| d == &root),
            "root .vst3 must be scanned"
        );
        assert!(
            dirs.iter().any(|d| d == &yabridge),
            "yabridge subfolder must be scanned"
        );
        assert!(!dirs.iter().any(|d| d == &bundle));

        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn scan_lists_yabridge_without_loading() {
        let dirs = standard_vst3_dirs();
        let plugins = scan_paths(&dirs).expect("scan");
        let yabridge = plugins
            .iter()
            .filter(|p| p.path.to_string_lossy().contains("yabridge"))
            .count();
        // Should be instant filesystem listing — no Wine.
        assert!(
            yabridge > 0
                || !PathBuf::from(std::env::var("HOME").unwrap())
                    .join(".vst3/yabridge")
                    .exists(),
            "expected yabridge plugins when ~/.vst3/yabridge exists"
        );
        eprintln!(
            "catalogued {} plugins ({} yabridge)",
            plugins.len(),
            yabridge
        );
    }
}
