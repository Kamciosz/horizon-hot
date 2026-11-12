//! PL: Lustro repo serwera w `hot-state/repo.git` przez program `git` (bez nowej zależności,
//! ADR 0008). Pobiera gałąź serwera i gałąź `hot-builds` z wynikami CI; pliki czyta prosto
//! z obiektów gita, bez katalogu roboczego. Dostęp do prywatnego repo daje klucz tylko do odczytu
//! (`GIT_SSH_COMMAND` w usłudze, ustawia instalator).
//!
//! EN: A mirror of the server repository in `hot-state/repo.git` through the `git` program (no new
//! dependency, ADR 0008). Fetches the server branch and the `hot-builds` branch with CI results and
//! reads files straight from git objects, with no work tree. A read-only deploy key gives access to
//! a private repository (`GIT_SSH_COMMAND` in the service, set by the installer).

use std::path::{Path, PathBuf};
use std::process::Command;

use hot_diag::DiagError;

use crate::codes;

/// PL: Gałąź z wynikami CI (D14, ADR 0008). EN: The branch with CI results (D14, ADR 0008).
pub const BUILDS_BRANCH: &str = "hot-builds";

/// PL: Lustro jednego repo. EN: A mirror of one repository.
pub struct Mirror {
    pub dir: PathBuf,
    pub url: String,
    pub branch: String,
}

impl Mirror {
    #[must_use]
    pub fn new(dir: PathBuf, url: &str, branch: &str) -> Self {
        Self {
            dir,
            url: url.to_owned(),
            branch: branch.to_owned(),
        }
    }

    /// PL: Tworzy lustro, jeśli go nie ma, i pobiera obie gałęzie. Brak `hot-builds` nie jest
    /// błędem: CI mogło jeszcze nic nie zbudować.
    /// EN: Creates the mirror if missing and fetches both branches. A missing `hot-builds` is not
    /// an error: CI may not have built anything yet.
    ///
    /// # Errors
    /// PL: `HOT-E0201`. EN: `HOT-E0201`.
    pub fn fetch(&self) -> Result<(), DiagError> {
        if !self.dir.join("HEAD").is_file() {
            run_git(
                None,
                &["init", "--bare", "--quiet", &self.dir.display().to_string()],
            )
            .map_err(|e| self.fetch_error(&e))?;
        }
        // PL: Jedno wywołanie po wszystkie gałęzie: `hot-builds` powstaje przy pierwszym
        // przebiegu CI, a wzorzec nie zgłasza błędu, gdy jej jeszcze nie ma.
        // EN: One call for every branch: `hot-builds` appears with the first CI run, and a pattern
        // does not fail while it is still missing.
        run_git(
            Some(&self.dir),
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--prune",
                &self.url,
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        )
        .map_err(|e| self.fetch_error(&e))?;
        Ok(())
    }

    fn fetch_error(&self, e: &GitError) -> DiagError {
        DiagError::new(
            codes::GIT_FETCH_FAILED,
            format!("hotd cannot fetch the server repository {}", self.url),
            "Run the same command by hand as the service user (sudo -u horizon git ls-remote <url>) and fix access: network, the deploy key in ~/.ssh or the repository address in the hotd service",
        )
        .expected("git fetch downloads the server branch and the hot-builds branch")
        .context("repo", self.url.clone())
        .context("branch", self.branch.clone())
        .context("mirror", self.dir.display().to_string())
        .context("command", e.command.clone())
        .chain_line(e.output.clone())
        .state("Nothing changes on the server; hotd tries again in 10 seconds")
    }

    /// PL: Commit na czubku gałęzi serwera. EN: The commit at the tip of the server branch.
    #[must_use]
    pub fn head(&self) -> Option<String> {
        self.rev(&format!("refs/remotes/origin/{}", self.branch))
    }

    /// PL: Pełny hash rewizji albo `None`. EN: The full hash of a revision, or `None`.
    #[must_use]
    pub fn rev(&self, rev: &str) -> Option<String> {
        let spec = format!("{rev}^{{commit}}");
        run_git(
            Some(&self.dir),
            &["rev-parse", "--verify", "--quiet", &spec],
        )
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
    }

    /// PL: Zmienione ścieżki `old..new`; bez `old` — wszystkie pliki `new`.
    /// EN: Changed paths `old..new`; without `old`, every file in `new`.
    ///
    /// # Errors
    /// PL: `HOT-E0201`, gdy git nie zna commitów. EN: `HOT-E0201` when git lacks the commits.
    pub fn changed(&self, old: Option<&str>, new: &str) -> Result<Vec<String>, DiagError> {
        let out = match old {
            Some(old) => run_git(
                Some(&self.dir),
                &["diff", "--name-only", "--no-renames", old, new],
            ),
            None => return self.files(new),
        }
        .map_err(|e| self.fetch_error(&e))?;
        Ok(lines(&out))
    }

    /// PL: Wszystkie pliki w commicie. EN: Every file in a commit.
    ///
    /// # Errors
    /// PL: `HOT-E0201`. EN: `HOT-E0201`.
    pub fn files(&self, commit: &str) -> Result<Vec<String>, DiagError> {
        let out = run_git(Some(&self.dir), &["ls-tree", "-r", "--name-only", commit])
            .map_err(|e| self.fetch_error(&e))?;
        Ok(lines(&out))
    }

    /// PL: Hash drzewa `commit:path`; `None`, gdy ścieżki nie ma. Ten sam hash = ta sama zawartość.
    /// EN: The tree hash of `commit:path`; `None` when the path is absent. Same hash = same content.
    #[must_use]
    pub fn tree(&self, commit: &str, path: &str) -> Option<String> {
        let spec = format!("{commit}:{path}");
        run_git(
            Some(&self.dir),
            &["rev-parse", "--verify", "--quiet", &spec],
        )
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
    }

    /// PL: Wypakowuje drzewo gita do katalogu (`git archive` + `tar`, oba są na serwerze).
    /// EN: Unpacks a git tree into a folder (`git archive` + `tar`, both are on the server).
    ///
    /// # Errors
    /// PL: Opis nieudanego polecenia. EN: The failed command and its output.
    pub fn extract(&self, tree: &str, into: &Path) -> Result<(), GitError> {
        let archive = into.with_extension("tar");
        let archive_arg = archive.display().to_string();
        run_git(
            Some(&self.dir),
            &["archive", "--format=tar", "-o", &archive_arg, tree],
        )?;
        let command = format!("tar -xf {archive_arg} -C {}", into.display());
        let out = Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(into)
            .output();
        let _ = std::fs::remove_file(&archive);
        match out {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(GitError {
                command,
                output: format!(
                    "exit {}: {}",
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            }),
            Err(e) => Err(GitError {
                command,
                output: format!("tar cannot be started: {e}"),
            }),
        }
    }

    /// PL: Plik z gałęzi `hot-builds`; `None`, gdy go nie ma.
    /// EN: A file from the `hot-builds` branch; `None` when it is absent.
    #[must_use]
    pub fn build_file(&self, path: &str) -> Option<Vec<u8>> {
        self.blob(&format!("refs/remotes/origin/{BUILDS_BRANCH}"), path)
    }

    /// PL: Treść pliku `path` w `rev`; `None`, gdy go nie ma. EN: The content of `path` at `rev`.
    #[must_use]
    pub fn blob(&self, rev: &str, path: &str) -> Option<Vec<u8>> {
        let spec = format!("{rev}:{path}");
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(&self.dir)
            .args(["cat-file", "blob", &spec])
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)
    }
}

fn lines(out: &str) -> Vec<String> {
    out.lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// PL: Nieudane wywołanie gita z komendą i wyjściem. EN: A failed git call with command and output.
#[derive(Debug)]
pub struct GitError {
    pub command: String,
    pub output: String,
}

fn run_git(dir: Option<&Path>, args: &[&str]) -> Result<String, GitError> {
    let mut cmd = Command::new("git");
    if let Some(dir) = dir {
        cmd.arg("--git-dir").arg(dir);
    }
    // PL: Bez pytań o hasło: usługa nie ma terminala. EN: No password prompts: no terminal.
    cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");
    let command = format!("git {}", args.join(" "));
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Ok(out) => Err(GitError {
            command,
            output: format!(
                "exit {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }),
        Err(e) => Err(GitError {
            command,
            output: format!("git cannot be started: {e}"),
        }),
    }
}
