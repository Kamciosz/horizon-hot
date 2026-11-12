//! PL: Rozmowa z silnikiem. Prawdziwy silnik to `horizon-server --dir <katalog> cmd <linia>`
//! i `status --json`; testy podstawiają własną implementację cechy [`Engine`].
//!
//! EN: Talking to the engine. The real engine is `horizon-server --dir <dir> cmd <line>` and
//! `status --json`; tests plug in their own [`Engine`] implementation.

use std::path::PathBuf;
use std::process::Command;

use hot_diag::DiagError;
use serde_json::Value;

use crate::codes;

/// PL: Co `hotd` potrzebuje od silnika. EN: What `hotd` needs from the engine.
pub trait Engine {
    /// PL: Komenda konsoli, np. `hot load <plik>`. EN: A console command, e.g. `hot load <file>`.
    ///
    /// # Errors
    /// PL: `HOT-E0205`. EN: `HOT-E0205`.
    fn command(&mut self, line: &str) -> Result<(), DiagError>;

    /// PL: PID działającego procesu silnika; `None`, gdy silnik nie działa.
    /// EN: The PID of the running engine process; `None` when the engine is not running.
    fn pid(&mut self) -> Option<u32>;
}

/// PL: `horizon-server` z katalogiem serwera. EN: `horizon-server` with a server folder.
pub struct HorizonServer {
    pub program: PathBuf,
    pub dir: PathBuf,
}

impl HorizonServer {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = Command::new(&self.program)
            .arg("--dir")
            .arg(&self.dir)
            .args(args)
            .output()
            .map_err(|e| format!("{} cannot be started: {e}", self.program.display()))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            Ok(stdout)
        } else {
            Err(format!(
                "exit {}: {} {}",
                out.status,
                stdout.trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

impl Engine for HorizonServer {
    fn command(&mut self, line: &str) -> Result<(), DiagError> {
        self.run(&["cmd", line]).map(drop).map_err(|output| {
            DiagError::new(
                codes::APPLY_FAILED,
                format!("hotd cannot send the console command \"{line}\" to the engine"),
                "Check that the server runs (horizon-server --dir <dir> status) and that hotd uses the same --engine-dir; then hotd retries with the next commit or after a restart",
            )
            .expected("horizon-server cmd delivers the command to the running engine")
            .context("program", self.program.display().to_string())
            .context("dir", self.dir.display().to_string())
            .context("command", line.to_owned())
            .chain_line(output)
            .state("The commit was not applied; the server keeps the mods it had")
        })
    }

    fn pid(&mut self) -> Option<u32> {
        let out = self.run(&["status", "--json"]).ok()?;
        let v: Value = serde_json::from_str(&out).ok()?;
        if v["phase"] != "running" {
            return None;
        }
        v["engine_pid"].as_u64().and_then(|p| u32::try_from(p).ok())
    }
}
