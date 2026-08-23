use crate::command::CommandSpec;
use crate::error::{invalid_input, message, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct ResolvedLaunch {
    pub system_id: String,
    pub emulator_id: String,
    pub emulator_name: String,
    pub rom_path: String,
    pub command: CommandSpec,
}

#[derive(Clone, Debug)]
pub struct Registry {
    systems: BTreeMap<String, System>,
    emulators: BTreeMap<String, Emulator>,
}

#[derive(Clone, Debug, Deserialize)]
struct SystemDocument {
    schema: u32,
    system: System,
}

#[derive(Clone, Debug, Deserialize)]
struct EmulatorDocument {
    schema: u32,
    emulator: Emulator,
}

#[derive(Clone, Debug, Deserialize)]
struct System {
    id: String,
    name: String,
    extensions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Emulator {
    id: String,
    name: String,
    version: String,
    systems: Vec<String>,
    priority: i64,
    exec: EmulatorExec,
}

#[derive(Clone, Debug, Deserialize)]
struct EmulatorExec {
    argv: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    workdir: Option<String>,
}

impl Registry {
    pub fn load() -> Result<Self> {
        let roots = registry_roots();
        let mut registry = Self {
            systems: BTreeMap::new(),
            emulators: BTreeMap::new(),
        };

        for root in roots {
            registry.load_layer(&root)?;
        }

        if registry.systems.is_empty() {
            return Err(message("Zaman registry contains no systems"));
        }
        if registry.emulators.is_empty() {
            return Err(message("Zaman registry contains no emulators"));
        }

        registry.validate_cross_references()?;
        Ok(registry)
    }

    pub fn resolve(&self, system_id: &str, rom_path: &str) -> Result<ResolvedLaunch> {
        validate_id(system_id, "system ID")?;
        let system = self
            .systems
            .get(system_id)
            .ok_or_else(|| invalid_input(format!("unknown system: {system_id}")))?;

        let supplied_path = Path::new(rom_path);
        if !supplied_path.is_absolute() {
            return Err(invalid_input(format!(
                "ROM path must be absolute: {rom_path}"
            )));
        }

        let canonical_path = fs::canonicalize(supplied_path)
            .map_err(|error| invalid_input(format!("cannot access ROM {rom_path}: {error}")))?;
        if !canonical_path.is_file() {
            return Err(invalid_input(format!(
                "ROM path is not a regular file: {}",
                canonical_path.display()
            )));
        }

        let extension = canonical_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| invalid_input("ROM has no usable extension"))?;
        if !system
            .extensions
            .iter()
            .any(|allowed| allowed == &extension)
        {
            return Err(invalid_input(format!(
                "extension .{extension} is not valid for system {system_id}"
            )));
        }

        let emulator = self.select_emulator(system_id)?;
        let tokens = token_values(system, emulator, &canonical_path)?;
        let argv = emulator
            .exec
            .argv
            .iter()
            .map(|argument| expand(argument, &tokens))
            .collect::<Result<Vec<_>>>()?;
        let environment = emulator
            .exec
            .env
            .iter()
            .map(|(key, value)| Ok((key.clone(), expand(value, &tokens)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let working_directory = emulator
            .exec
            .workdir
            .as_deref()
            .map(|path| expand(path, &tokens))
            .transpose()?;
        let command = CommandSpec::new(argv, environment, working_directory)?;

        Ok(ResolvedLaunch {
            system_id: system.id.clone(),
            emulator_id: emulator.id.clone(),
            emulator_name: emulator.name.clone(),
            rom_path: canonical_path.to_string_lossy().into_owned(),
            command,
        })
    }

    fn load_layer(&mut self, root: &Path) -> Result<()> {
        self.load_systems(&root.join("systems.d"))?;
        self.load_emulators(&root.join("emulators.d"))?;
        Ok(())
    }

    fn load_systems(&mut self, directory: &Path) -> Result<()> {
        let mut seen = BTreeSet::new();
        for path in toml_files(directory)? {
            let document: SystemDocument = read_toml(&path)?;
            if document.schema != SCHEMA_VERSION {
                return Err(message(format!(
                    "{} uses registry schema {}, expected {SCHEMA_VERSION}",
                    path.display(),
                    document.schema
                )));
            }
            validate_system(&document.system, &path)?;
            if !seen.insert(document.system.id.clone()) {
                return Err(message(format!(
                    "duplicate system {} in registry layer {}",
                    document.system.id,
                    directory.display()
                )));
            }
            self.systems
                .insert(document.system.id.clone(), document.system);
        }
        Ok(())
    }

    fn load_emulators(&mut self, directory: &Path) -> Result<()> {
        let mut seen = BTreeSet::new();
        for path in toml_files(directory)? {
            let document: EmulatorDocument = read_toml(&path)?;
            if document.schema != SCHEMA_VERSION {
                return Err(message(format!(
                    "{} uses registry schema {}, expected {SCHEMA_VERSION}",
                    path.display(),
                    document.schema
                )));
            }
            validate_emulator(&document.emulator, &path)?;
            if !seen.insert(document.emulator.id.clone()) {
                return Err(message(format!(
                    "duplicate emulator {} in registry layer {}",
                    document.emulator.id,
                    directory.display()
                )));
            }
            self.emulators
                .insert(document.emulator.id.clone(), document.emulator);
        }
        Ok(())
    }

    fn validate_cross_references(&self) -> Result<()> {
        let mut candidates: BTreeMap<&str, Vec<&Emulator>> = BTreeMap::new();
        for emulator in self.emulators.values() {
            for system_id in &emulator.systems {
                if !self.systems.contains_key(system_id) {
                    return Err(message(format!(
                        "emulator {} claims unknown system {system_id}",
                        emulator.id
                    )));
                }
                candidates.entry(system_id).or_default().push(emulator);
            }
        }

        for system_id in self.systems.keys() {
            let system_candidates = candidates
                .get(system_id.as_str())
                .ok_or_else(|| message(format!("system {system_id} has no emulator")))?;
            reject_priority_tie(system_id, system_candidates)?;
        }
        Ok(())
    }

    fn select_emulator(&self, system_id: &str) -> Result<&Emulator> {
        let candidates: Vec<&Emulator> = self
            .emulators
            .values()
            .filter(|emulator| emulator.systems.iter().any(|system| system == system_id))
            .collect();
        reject_priority_tie(system_id, &candidates)?;
        candidates
            .into_iter()
            .max_by_key(|emulator| emulator.priority)
            .ok_or_else(|| message(format!("system {system_id} has no emulator")))
    }
}

fn registry_roots() -> Vec<PathBuf> {
    env::var_os("ZAMAN_REGISTRY_DIRS")
        .map(|paths| env::split_paths(&paths).collect())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/share/zaman"),
                PathBuf::from("/etc/zaman"),
            ]
        })
}

fn toml_files(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(directory)
        .map_err(|error| message(format!("cannot read {}: {error}", directory.display())))?
        .filter_map(|entry| match entry {
            Ok(entry)
                if entry.path().extension().and_then(|value| value.to_str()) == Some("toml") =>
            {
                Some(Ok(entry.path()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

fn read_toml<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let source = fs::read_to_string(path)?;
    Ok(toml::from_str(&source)
        .map_err(|error| message(format!("invalid registry file {}: {error}", path.display())))?)
}

fn validate_system(system: &System, path: &Path) -> Result<()> {
    validate_id(&system.id, "system ID")?;
    if system.name.is_empty() || system.extensions.is_empty() {
        return Err(message(format!(
            "{} has an incomplete system definition",
            path.display()
        )));
    }
    for extension in &system.extensions {
        if extension.is_empty()
            || extension.starts_with('.')
            || extension
                .bytes()
                .any(|byte| !byte.is_ascii_lowercase() && !byte.is_ascii_digit())
        {
            return Err(message(format!(
                "{} has invalid extension {extension}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn validate_emulator(emulator: &Emulator, path: &Path) -> Result<()> {
    validate_id(&emulator.id, "emulator ID")?;
    if emulator.name.is_empty()
        || emulator.version.is_empty()
        || emulator.systems.is_empty()
        || emulator.exec.argv.is_empty()
    {
        return Err(message(format!(
            "{} has an incomplete emulator definition",
            path.display()
        )));
    }
    if !Path::new(&emulator.exec.argv[0]).is_absolute() {
        return Err(message(format!(
            "{} emulator argv[0] must be absolute",
            path.display()
        )));
    }
    for system_id in &emulator.systems {
        validate_id(system_id, "emulator system ID")?;
    }
    Ok(())
}

fn validate_id(value: &str, description: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'-' => index > 0,
            _ => false,
        });
    if !valid || value.ends_with('-') {
        return Err(invalid_input(format!("invalid {description}: {value}")));
    }
    Ok(())
}

fn reject_priority_tie(system_id: &str, candidates: &[&Emulator]) -> Result<()> {
    let Some(highest) = candidates.iter().map(|emulator| emulator.priority).max() else {
        return Ok(());
    };
    let winners: Vec<&str> = candidates
        .iter()
        .filter(|emulator| emulator.priority == highest)
        .map(|emulator| emulator.id.as_str())
        .collect();
    if winners.len() > 1 {
        return Err(message(format!(
            "priority tie for system {system_id}: {}",
            winners.join(", ")
        )));
    }
    Ok(())
}

fn token_values(
    system: &System,
    emulator: &Emulator,
    rom_path: &Path,
) -> Result<BTreeMap<&'static str, String>> {
    let rom_path_string = rom_path.to_string_lossy().into_owned();
    let rom_basename = rom_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid_input("ROM basename is not valid UTF-8"))?
        .to_string();
    let rom_id = rom_path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid_input("ROM ID is not valid UTF-8"))?
        .to_string();

    let home = env::var("HOME").map_err(|_| message("HOME is not set"))?;
    let data_home = env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
    let state_home = env::var("XDG_STATE_HOME").unwrap_or_else(|_| format!("{home}/.local/state"));
    let config_home = env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
    let cache_home = env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{home}/.cache"));

    Ok(BTreeMap::from([
        ("rom.path", rom_path_string),
        ("rom.id", rom_id),
        ("rom.basename", rom_basename),
        ("system.id", system.id.clone()),
        (
            "system.savedir",
            format!("{data_home}/zaman/saves/{}", system.id),
        ),
        (
            "system.statedir",
            format!("{state_home}/zaman/{}", system.id),
        ),
        ("system.biosdir", format!("/srv/zaman/bios/{}", system.id)),
        (
            "emulator.configdir",
            format!("{config_home}/zaman/emulators/{}", emulator.id),
        ),
        (
            "emulator.datadir",
            format!("{data_home}/zaman/emulators/{}", emulator.id),
        ),
        (
            "emulator.cachedir",
            format!("{cache_home}/zaman/emulators/{}", emulator.id),
        ),
    ]))
}

fn expand(template: &str, tokens: &BTreeMap<&str, String>) -> Result<String> {
    let mut output = String::with_capacity(template.len());
    let mut remainder = template;

    while let Some(open) = remainder.find('{') {
        output.push_str(&remainder[..open]);
        let after_open = &remainder[open + 1..];
        let close = after_open
            .find('}')
            .ok_or_else(|| message(format!("unclosed registry token in {template:?}")))?;
        let token = &after_open[..close];
        let value = tokens
            .get(token)
            .ok_or_else(|| message(format!("unknown registry token {{{token}}}")))?;
        output.push_str(value);
        remainder = &after_open[close + 1..];
    }

    if remainder.contains('}') {
        return Err(message(format!(
            "unmatched closing brace in registry value {template:?}"
        )));
    }
    output.push_str(remainder);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{Emulator, EmulatorExec, Registry, System};
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_registry(priorities: &[i64]) -> Registry {
        let system = System {
            id: "nes".to_string(),
            name: "Nintendo Entertainment System".to_string(),
            extensions: vec!["nes".to_string()],
        };
        let executable = std::env::current_exe()
            .expect("test executable path")
            .to_string_lossy()
            .into_owned();
        let emulators = priorities
            .iter()
            .enumerate()
            .map(|(index, priority)| {
                let id = format!("emulator-{index}");
                (
                    id.clone(),
                    Emulator {
                        id,
                        name: format!("Emulator {index}"),
                        version: "1".to_string(),
                        systems: vec!["nes".to_string()],
                        priority: *priority,
                        exec: EmulatorExec {
                            argv: vec![executable.clone(), "{rom.path}".to_string()],
                            env: BTreeMap::new(),
                            workdir: None,
                        },
                    },
                )
            })
            .collect();
        Registry {
            systems: BTreeMap::from([("nes".to_string(), system)]),
            emulators,
        }
    }

    fn test_rom() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("zaman ROM {unique}.nes"));
        fs::write(&path, b"test").expect("write test ROM");
        path
    }

    #[test]
    fn selects_highest_priority_and_preserves_rom_spaces() {
        let registry = test_registry(&[10, 20]);
        let rom = test_rom();
        let launch = registry
            .resolve("nes", rom.to_str().expect("UTF-8 path"))
            .expect("resolved launch");

        assert_eq!(launch.emulator_id, "emulator-1");
        assert_eq!(launch.command.argv()[1], rom.to_string_lossy());
        fs::remove_file(rom).expect("remove test ROM");
    }

    #[test]
    fn rejects_top_priority_ties() {
        let registry = test_registry(&[20, 20]);
        let rom = test_rom();
        let result = registry.resolve("nes", rom.to_str().expect("UTF-8 path"));

        assert!(result.is_err());
        fs::remove_file(rom).expect("remove test ROM");
    }

    #[test]
    fn rejects_wrong_extension() {
        let registry = test_registry(&[20]);
        let original = test_rom();
        let rom = original.with_extension("zip");
        fs::write(&rom, b"test").expect("write test ROM");
        let result = registry.resolve("nes", rom.to_str().expect("UTF-8 path"));

        assert!(result.is_err());
        fs::remove_file(original).expect("remove original test ROM");
        fs::remove_file(rom).expect("remove test ROM");
    }
}
