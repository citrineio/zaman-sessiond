use crate::error::{invalid_input, Result};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    argv: Vec<String>,
    environment: Vec<String>,
    working_directory: Option<String>,
}

impl CommandSpec {
    pub fn new(
        argv: Vec<String>,
        registry_environment: BTreeMap<String, String>,
        working_directory: Option<String>,
    ) -> Result<Self> {
        let executable = argv
            .first()
            .ok_or_else(|| invalid_input("emulator argv must not be empty"))?;
        let executable_path = Path::new(executable);

        if !executable_path.is_absolute() {
            return Err(invalid_input(format!(
                "executable must be an absolute path: {executable}"
            )));
        }

        let metadata = fs::metadata(executable_path).map_err(|error| {
            invalid_input(format!("cannot access executable {executable}: {error}"))
        })?;

        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(invalid_input(format!(
                "not an executable file: {executable}"
            )));
        }

        if let Some(directory) = working_directory.as_deref() {
            let path = Path::new(directory);
            if !path.is_absolute() || !path.is_dir() {
                return Err(invalid_input(format!(
                    "working directory must be an existing absolute directory: {directory}"
                )));
            }
        }

        let mut environment = forwarded_session_environment();
        for (key, value) in registry_environment {
            validate_environment_entry(&key, &value)?;
            environment.insert(key, value);
        }

        Ok(Self {
            argv,
            environment: environment
                .into_iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect(),
            working_directory,
        })
    }

    pub fn executable(&self) -> &str {
        &self.argv[0]
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn environment(&self) -> &[String] {
        &self.environment
    }

    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }
}

const FORWARDED_ENVIRONMENT: &[&str] = &[
    "DBUS_SESSION_BUS_ADDRESS",
    "DISPLAY",
    "LANG",
    "LC_ALL",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
];

fn forwarded_session_environment() -> BTreeMap<String, String> {
    FORWARDED_ENVIRONMENT
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect()
}

fn validate_environment_entry(key: &str, value: &str) -> Result<()> {
    let valid_key = !key.is_empty()
        && key.bytes().enumerate().all(|(index, byte)| match byte {
            b'A'..=b'Z' | b'_' => true,
            b'0'..=b'9' => index > 0,
            _ => false,
        });

    if !valid_key || value.contains('\0') || value.contains('\n') {
        return Err(invalid_input(format!(
            "invalid emulator environment entry: {key}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CommandSpec;
    use std::collections::BTreeMap;

    #[test]
    fn accepts_an_absolute_executable_and_preserves_argv() {
        let executable = std::env::current_exe()
            .expect("test executable path")
            .to_string_lossy()
            .into_owned();
        let expected = vec![executable, "ROM with spaces.nes".to_string()];

        let command =
            CommandSpec::new(expected.clone(), BTreeMap::new(), None).expect("valid command");

        assert_eq!(command.argv(), expected);
    }

    #[test]
    fn rejects_a_relative_executable() {
        let argv = vec!["sleep".to_string(), "infinity".to_string()];

        assert!(CommandSpec::new(argv, BTreeMap::new(), None).is_err());
    }

    #[test]
    fn rejects_bad_environment_keys() {
        let executable = std::env::current_exe()
            .expect("test executable path")
            .to_string_lossy()
            .into_owned();
        let mut environment = BTreeMap::new();
        environment.insert("bad-key".to_string(), "value".to_string());

        assert!(CommandSpec::new(vec![executable], environment, None).is_err());
    }
}
