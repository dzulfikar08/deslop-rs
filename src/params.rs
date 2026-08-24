//! Ports `Params.hs` — validated run parameters.

use camino::{Utf8Path, Utf8PathBuf};

use crate::types::DeslopError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Check,
    Fix,
    Baseline,
}

#[derive(Debug, Clone)]
pub struct ParamsDto {
    pub command: Command,
    pub project_dir: String,
}

#[derive(Debug, Clone)]
pub struct Params {
    pub project_path: Utf8PathBuf,
    pub command: Command,
}

impl Params {
    /// Makes PROJECT_DIR absolute against the process CWD.
    pub fn from_dto(dto: &ParamsDto) -> Result<Self, DeslopError> {
        let p = Utf8Path::new(&dto.project_dir);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir()
                .ok()
                .and_then(|cwd| Utf8PathBuf::from_path_buf(cwd).ok())
                .ok_or_else(|| DeslopError::InvalidRuleConfig("non-UTF8 cwd".into()))?
                .join(p)
        };
        Ok(Self { project_path: abs, command: dto.command })
    }
}
