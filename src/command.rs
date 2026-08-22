use crate::error::{invalid_input, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    argv: Vec<String>,
}

impl CommandSpec {
    pub fn from_env() -> Result<Self> {
        Self::parse(std::env::args())
    }

    fn parse<I>(args: I) -> Result<Self>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = args.into_iter();
        let program = args.next().unwrap_or_else(|| "zaman-sessiond".to_string());

        if args.next().as_deref() != Some("--") {
            return Err(invalid_input(format!(
                "usage: {program} -- /absolute/executable [arguments...]"
            )));
        }

        let argv: Vec<String> = args.collect();
        let executable = argv
            .first()
            .ok_or_else(|| invalid_input("an executable must follow --"))?;
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

        Ok(Self { argv })
    }

    pub fn executable(&self) -> &str {
        &self.argv[0]
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

#[cfg(test)]
mod tests {
    use super::CommandSpec;

    #[test]
    fn accepts_an_absolute_executable_and_preserves_argv() {
        let executable = std::env::current_exe()
            .expect("test executable path")
            .to_string_lossy()
            .into_owned();
        let expected = vec![executable.clone(), "ROM with spaces.nes".to_string()];
        let args = vec![
            "zaman-sessiond".to_string(),
            "--".to_string(),
            executable,
            "ROM with spaces.nes".to_string(),
        ];

        let command = CommandSpec::parse(args).expect("valid command");

        assert_eq!(command.argv(), expected);
    }

    #[test]
    fn rejects_a_relative_executable() {
        let args = vec![
            "zaman-sessiond".to_string(),
            "--".to_string(),
            "sleep".to_string(),
            "infinity".to_string(),
        ];

        assert!(CommandSpec::parse(args).is_err());
    }

    #[test]
    fn requires_the_command_separator() {
        let args = vec!["zaman-sessiond".to_string(), "/bin/true".to_string()];

        assert!(CommandSpec::parse(args).is_err());
    }
}
