use anyhow::Result;
use is_terminal::IsTerminal;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    Json,
}

impl OutputMode {
    pub fn resolve(force_json: bool, force_human: bool) -> Self {
        if force_json || (!force_human && !std::io::stdout().is_terminal()) {
            Self::Json
        } else {
            Self::Human
        }
    }

    pub fn print<T: Serialize>(&self, human: impl AsRef<str>, payload: &T) -> Result<()> {
        match self {
            Self::Human => println!("{}", human.as_ref()),
            Self::Json => println!("{}", serde_json::to_string(payload)?),
        }
        Ok(())
    }
}
