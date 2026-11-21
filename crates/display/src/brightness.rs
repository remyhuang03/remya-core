//! Hardware brightness: kernel backlight through logind, external displays through
//! the optional `ddcutil` command. Values are live state and are never persisted.
use crate::hyprland::run;
use gio::glib::variant::ToVariant;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

const DDC_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Brightness {
    pub output: String,
    pub percent: Option<u32>,
    pub backend: Option<&'static str>,
    pub raw_value: Option<u32>,
    pub raw_max: Option<u32>,
    pub safe_min_raw: Option<u32>,
    pub reason: String,
}

impl Brightness {
    fn observed(output: &str, backend: &'static str, value: u32, max: u32, floor: u32) -> Self {
        Self {
            output: output.to_owned(),
            percent: (value > 0).then(|| percent(value, max, floor)),
            backend: Some(backend),
            raw_value: Some(value),
            raw_max: Some(max),
            safe_min_raw: Some(floor),
            reason: if value == 0 {
                "The display reports a raw hardware brightness of zero".into()
            } else {
                String::new()
            },
        }
    }

    fn unavailable(output: &str, backend: Option<&'static str>, reason: String) -> Self {
        Self {
            output: output.to_owned(),
            percent: None,
            backend,
            raw_value: None,
            raw_max: None,
            safe_min_raw: None,
            reason,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Backlight {
    name: String,
    connector: Option<String>,
}

impl Backlight {
    fn safe_min_raw(&self, root: &Path, max: u32) -> u32 {
        backlight_floor(
            &root.join(&self.name),
            max,
            env::var_os("REMYA_BACKLIGHT_RAW_ONE").as_deref(),
        )
    }
}

fn strip_card(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("card")?;
    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
    (digits > 0).then(|| rest[digits..].trim_start_matches('-'))
}

fn backlights(root: &Path) -> Vec<Backlight> {
    let Ok(entries) = fs::read_dir(root) else {
        return vec![];
    };
    let mut list = entries
        .flatten()
        .map(|entry| Backlight {
            name: entry.file_name().to_string_lossy().into_owned(),
            connector: fs::canonicalize(entry.path().join("device"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .and_then(|n| strip_card(&n).map(str::to_owned)),
        })
        .collect::<Vec<_>>();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

fn is_internal(output: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|p| output.starts_with(p))
}

/// Connector-linked devices win; a firmware device only serves a single internal panel.
fn backlight_for<'a>(
    list: &'a [Backlight],
    output: &str,
    outputs: &[String],
) -> Option<&'a Backlight> {
    list.iter()
        .find(|b| b.connector.as_deref() == Some(output))
        .or_else(|| {
            let internal = outputs.iter().filter(|o| is_internal(o)).count();
            (is_internal(output) && internal == 1)
                .then(|| list.iter().find(|b| b.connector.is_none()))
                .flatten()
        })
}

fn read_number(path: PathBuf) -> Result<u32, String> {
    fs::read_to_string(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .trim()
        .parse()
        .map_err(|_| format!("Invalid value in {}", path.display()))
}

fn read_backlight(root: &Path, name: &str) -> Result<(u32, u32), String> {
    let dir = root.join(name);
    let max = read_number(dir.join("max_brightness"))?;
    if max == 0 {
        return Err(format!("{name} reports no brightness range"));
    }
    let value = read_number(dir.join("actual_brightness"))?;
    if value > max {
        return Err(format!(
            "{name} reports raw brightness {value} above maximum {max}"
        ));
    }
    Ok((value, max))
}

fn percent(value: u32, max: u32, floor: u32) -> u32 {
    if value == floor {
        return 0;
    }
    ((value as u64 * 100 + max as u64 / 2) / max as u64).clamp(1, 100) as u32
}

fn conservative_floor(max: u32) -> u32 {
    ((max as u64 + 50) / 100).max(1) as u32
}

fn backlight_floor(device: &Path, max: u32, approved: Option<&OsStr>) -> u32 {
    if let Some(approved) = approved
        && fs::canonicalize(device).is_ok_and(|path| path.as_os_str() == approved)
    {
        return 1;
    }
    conservative_floor(max)
}

fn raw_value(wanted: u32, max: u32, floor: u32, round: u32) -> u32 {
    if wanted == 0 {
        return floor;
    }
    let raw = ((wanted.min(100) as u64 * max as u64 + round as u64) / 100) as u32;
    // A positive request has to leave the floor even on devices with few steps.
    raw.max(floor.saturating_add(1).min(max))
}

fn backlight_matches(value: u32, raw: u32, max: u32, floor: u32, exact: bool) -> bool {
    if exact || raw == floor {
        return value == raw;
    }
    value > floor && value.abs_diff(raw) <= max / 100 + 1
}

fn write_with_restore(
    raw: u32,
    previous: u32,
    floor: u32,
    wanted: u32,
    mut write: impl FnMut(u32, bool) -> Result<u32, String>,
) -> Result<u32, String> {
    if raw == previous {
        return Ok(previous);
    }
    let result = write(raw, wanted == 0);
    if wanted != 0 {
        return result;
    }
    result.map_err(|error| {
        let restore = if previous > 0 { previous } else { floor };
        match write(restore, true) {
            Ok(actual) => format!("{error}; restored hardware brightness to raw {actual}"),
            Err(rollback) => format!("{error}; could not restore raw {restore}: {rollback}"),
        }
    })
}

fn logind_set(name: &str, value: u32) -> Result<(), String> {
    let bus = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE)
        .map_err(|e| e.to_string())?;
    bus.call_sync(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
        "SetBrightness",
        Some(&("backlight", name, value).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        5000,
        gio::Cancellable::NONE,
    )
    .map_err(|e| format!("logind SetBrightness: {e}"))?;
    Ok(())
}

/// Connector name to I2C bus, from `ddcutil detect --terse`. Invalid entries are skipped.
fn parse_detect(text: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for block in text.split("\n\n") {
        if !block.trim_start().starts_with("Display ") {
            continue;
        }
        let value = |key: &str| {
            block
                .lines()
                .find_map(|line| line.trim().strip_prefix(key).map(|v| v.trim().to_owned()))
        };
        let bus = value("I2C bus:").and_then(|b| b.rsplit('-').next().map(str::to_owned));
        let connector = value("DRM connector:")
            .or_else(|| value("DRM_connector:"))
            .and_then(|c| strip_card(&c).map(str::to_owned));
        if let (Some(bus), Some(connector)) = (bus, connector) {
            found.insert(connector, bus);
        }
    }
    found
}

/// Reads `VCP 10 C <current> <max>`.
fn parse_vcp(text: &str) -> Option<(u32, u32)> {
    let line = text.lines().find(|l| l.starts_with("VCP 10 "))?;
    let parts = line.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        ["VCP", "10", "C", current, max] => {
            let max = max.parse().ok().filter(|m| *m > 0)?;
            let current = current.parse().ok().filter(|value| *value <= max)?;
            Some((current, max))
        }
        _ => None,
    }
}

fn ddc_read(bus: &str) -> Result<(u32, u32), String> {
    let text = run(
        "ddcutil",
        &["getvcp", "10", "--brief", "--bus", bus],
        DDC_TIMEOUT,
    )?;
    parse_vcp(&text).ok_or_else(|| "This display does not report DDC/CI brightness".to_owned())
}

fn verify_ddc_readback(
    (value, read_max): (u32, u32),
    target: u32,
    max: u32,
) -> Result<u32, String> {
    if read_max != max || value != target {
        return Err(format!(
            "DDC/CI reports raw {value}/{read_max}, expected raw {target}/{max}"
        ));
    }
    Ok(value)
}

/// Detection is slow and touches the I2C buses, so results are kept until the set
/// of connected outputs changes or the user asks to detect displays again.
#[derive(Default)]
pub struct BrightnessBackend {
    ddc: Option<Result<BTreeMap<String, String>, String>>,
}

impl BrightnessBackend {
    pub fn forget_detection(&mut self) {
        self.ddc = None;
    }

    fn ddc_buses(&mut self) -> Result<&BTreeMap<String, String>, String> {
        let result = self.ddc.get_or_insert_with(|| {
            run("ddcutil", &["detect", "--terse"], DDC_TIMEOUT).map(|text| parse_detect(&text))
        });
        match result {
            Ok(map) => Ok(map),
            Err(error) if error.contains("No such file") => {
                Err("Install ddcutil to control external display brightness".into())
            }
            Err(error) => Err(error.clone()),
        }
    }

    pub fn read(&mut self, output: &str, outputs: &[String]) -> Brightness {
        self.read_from(Path::new("/sys/class/backlight"), output, outputs)
    }

    pub(crate) fn read_backlight(output: &str, outputs: &[String]) -> Option<Brightness> {
        Self::read_backlight_from(Path::new("/sys/class/backlight"), output, outputs)
    }

    fn read_backlight_from(root: &Path, output: &str, outputs: &[String]) -> Option<Brightness> {
        let list = backlights(root);
        let device = backlight_for(&list, output, outputs)?;
        Some(match read_backlight(root, &device.name) {
            Ok((value, max)) => {
                let floor = device.safe_min_raw(root, max);
                Brightness::observed(output, "backlight", value, max, floor)
            }
            Err(error) => Brightness::unavailable(output, Some("backlight"), error),
        })
    }

    fn read_from(&mut self, root: &Path, output: &str, outputs: &[String]) -> Brightness {
        let unavailable = |reason| Brightness::unavailable(output, None, reason);
        if let Some(backlight) = Self::read_backlight_from(root, output, outputs) {
            return backlight;
        }
        if is_internal(output) {
            return unavailable("This panel has no kernel backlight device".into());
        }
        let bus = match self.ddc_buses() {
            Ok(map) => map.get(output).cloned(),
            Err(error) => return unavailable(error),
        };
        let Some(bus) = bus else {
            return unavailable("This display does not support DDC/CI".into());
        };
        match ddc_read(&bus) {
            Ok((value, max)) => {
                Brightness::observed(output, "ddc", value, max, conservative_floor(max))
            }
            Err(error) => unavailable(error),
        }
    }

    pub fn set(
        &mut self,
        output: &str,
        outputs: &[String],
        wanted: u32,
    ) -> Result<Brightness, String> {
        let root = Path::new("/sys/class/backlight");
        let list = backlights(root);
        if let Some(device) = backlight_for(&list, output, outputs) {
            let (previous, max) = read_backlight(root, &device.name)?;
            let floor = device.safe_min_raw(root, max);
            let raw = raw_value(wanted, max, floor, 50);
            let actual = write_with_restore(
                raw,
                previous,
                conservative_floor(max),
                wanted,
                |target, exact| {
                    logind_set(&device.name, target)?;
                    let mut actual = previous;
                    for _ in 0..20 {
                        let (value, read_max) = read_backlight(root, &device.name)?;
                        if read_max != max {
                            return Err(format!(
                                "{} changed its raw brightness range from {max} to {read_max}",
                                device.name
                            ));
                        }
                        if backlight_matches(value, target, max, floor, exact) {
                            return Ok(value);
                        }
                        actual = value;
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(format!(
                        "Backlight reports raw {actual}, expected raw {target}"
                    ))
                },
            )?;
            return Ok(Brightness::observed(
                output,
                "backlight",
                actual,
                max,
                floor,
            ));
        }
        if is_internal(output) {
            return Err("This panel has no kernel backlight device".into());
        }
        let bus = self
            .ddc_buses()?
            .get(output)
            .cloned()
            .ok_or("This display does not support DDC/CI")?;
        let (previous, max) = ddc_read(&bus)?;
        let floor = conservative_floor(max);
        let raw = raw_value(wanted, max, floor, 0);
        let actual = write_with_restore(raw, previous, floor, wanted, |target, _| {
            run(
                "ddcutil",
                &["setvcp", "10", &target.to_string(), "--bus", &bus],
                DDC_TIMEOUT,
            )?;
            verify_ddc_readback(ddc_read(&bus)?, target, max)
        })?;
        Ok(Brightness::observed(output, "ddc", actual, max, floor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_output_maps_connectors_and_skips_invalid_displays() {
        let text = "Invalid display\n   I2C bus:          /dev/i2c-12\n   DRM connector:    card0-eDP-1\n\nDisplay 1\n   I2C bus:          /dev/i2c-13\n   DRM connector:    card0-DP-1\n   Monitor:          YCT:Sculptor:\n\nDisplay 2\n   I2C bus:          /dev/i2c-15\n   DRM_connector:    card1-HDMI-A-1\n";
        let map = parse_detect(text);
        assert_eq!(map.len(), 2);
        assert_eq!(map["DP-1"], "13");
        assert_eq!(map["HDMI-A-1"], "15");
    }

    #[test]
    fn vcp_output_is_parsed_strictly() {
        assert_eq!(parse_vcp("VCP 10 C 50 100\n"), Some((50, 100)));
        assert_eq!(parse_vcp("VCP 10 ERR\n"), None);
        assert_eq!(parse_vcp("VCP 10 C 5 0"), None);
        assert_eq!(parse_vcp("VCP 10 C 11 10"), None);
    }

    #[test]
    fn percentages_keep_the_panel_lit() {
        assert_eq!(raw_value(0, 19200, 1, 50), 1);
        assert_eq!(raw_value(1, 19200, 1, 50), 192);
        assert_eq!(raw_value(100, 19200, 1, 50), 19200);
        assert_eq!(raw_value(1, 10, 1, 50), 2);
        assert_eq!(raw_value(0, 1, 1, 50), 1);
        assert_eq!(raw_value(100, 1, 1, 50), 1);
        assert_eq!(percent(1, 19200, 1), 0);
        assert_eq!(percent(2, 19200, 1), 1);
        assert_eq!(percent(7680, 19200, 1), 40);
        assert_eq!(percent(1, 1, 1), 0);
    }

    #[test]
    fn only_an_approved_controller_uses_raw_one() {
        let device = tempfile::tempdir().unwrap();
        let canonical = device.path().canonicalize().unwrap();
        assert_eq!(
            backlight_floor(device.path(), 19200, Some(canonical.as_os_str())),
            1
        );
        assert_eq!(
            backlight_floor(device.path(), 19200, Some(OsStr::new("other"))),
            192
        );
        assert_eq!(backlight_floor(device.path(), 19200, None), 192);
    }

    #[test]
    fn the_floor_requires_exact_readback() {
        assert!(backlight_matches(1, 1, 19200, 1, true));
        assert!(!backlight_matches(0, 1, 19200, 1, true));
        assert!(!backlight_matches(192, 1, 19200, 1, true));
        assert!(!backlight_matches(1, 192, 19200, 1, false));
        assert!(backlight_matches(190, 192, 19200, 1, false));
        assert!(!backlight_matches(191, 192, 19200, 192, false));
        let off = Brightness::observed("eDP-1", "backlight", 0, 19200, 1);
        assert_eq!(off.raw_value, Some(0));
        assert_eq!(off.percent, None);
    }

    #[test]
    fn floor_failure_restores_the_last_positive_raw_brightness() {
        let mut commands = vec![];
        let error = write_with_restore(1, 400, 192, 0, |target, exact| {
            assert!(exact);
            commands.push(target);
            if target == 1 {
                Err("Backlight reports raw 192, expected raw 1".into())
            } else {
                Ok(target)
            }
        })
        .unwrap_err();
        assert_eq!(commands, [1, 400]);
        assert!(error.contains("reports raw 192"));
        assert!(error.contains("restored hardware brightness to raw 400"));
    }

    #[test]
    fn ddc_never_targets_zero_and_only_reports_an_exact_floor() {
        for max in [1, 10, 100, 1000, u32::MAX] {
            let floor = conservative_floor(max);
            assert!((1..=max).contains(&floor));
            assert_eq!(raw_value(0, max, floor, 0), floor);
            assert_eq!(percent(floor, max, floor), 0);
            if floor < max {
                assert!(raw_value(1, max, floor, 0) > floor);
                assert!(percent(floor + 1, max, floor) > 0);
            }
        }
        assert_eq!(verify_ddc_readback((2, 200), 2, 200), Ok(2));
        assert!(verify_ddc_readback((1, 200), 2, 200).is_err());
        assert!(verify_ddc_readback((3, 200), 2, 200).is_err());
        assert!(verify_ddc_readback((2, 100), 2, 200).is_err());
    }

    #[test]
    fn backlights_resolve_through_sysfs_links() {
        let root = tempfile::tempdir().unwrap();
        let drm = root.path().join("drm/card0-eDP-1");
        let class = root.path().join("class");
        fs::create_dir_all(&drm).unwrap();
        for (name, link) in [("intel", Some(&drm)), ("acpi", None)] {
            let dir = class.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("max_brightness"), "200\n").unwrap();
            fs::write(dir.join("brightness"), "52\n").unwrap();
            fs::write(dir.join("actual_brightness"), "50\n").unwrap();
            if let Some(link) = link {
                std::os::unix::fs::symlink(link, dir.join("device")).unwrap();
            }
        }
        let outputs = vec!["eDP-1".to_owned(), "eDP-2".to_owned()];
        let list = backlights(&class);
        assert_eq!(
            backlight_for(&list, "eDP-1", &outputs).unwrap().name,
            "intel"
        );
        // Two internal panels: a firmware device cannot be attributed to either.
        assert!(backlight_for(&list, "eDP-2", &outputs).is_none());
        let one = vec!["LVDS-1".to_owned()];
        assert_eq!(backlight_for(&list, "LVDS-1", &one).unwrap().name, "acpi");
        let mut backend = BrightnessBackend::default();
        let read = backend.read_from(&class, "eDP-1", &outputs);
        assert_eq!(read.percent, Some(25));
        assert_eq!(read.backend, Some("backlight"));
        assert_eq!(read.raw_value, Some(50));
        assert_eq!(read.raw_max, Some(200));
        assert_eq!(read.safe_min_raw, Some(2));
        fs::write(class.join("intel/max_brightness"), "0\n").unwrap();
        let invalid = backend.read_from(&class, "eDP-1", &outputs);
        assert_eq!(invalid.percent, None);
        assert!(invalid.reason.contains("no brightness range"));
        let missing = backend.read_from(&class, "eDP-2", &outputs);
        assert_eq!(missing.percent, None);
        assert!(missing.reason.contains("no kernel backlight"));
    }
}
