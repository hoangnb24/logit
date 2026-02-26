use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    pub home_dir: PathBuf,
    pub cwd: PathBuf,
    pub out_dir: PathBuf,
}

pub fn resolve_runtime_paths(
    home_dir: &Path,
    cwd: &Path,
    out_dir_override: Option<&Path>,
) -> Result<RuntimePaths> {
    if !home_dir.is_absolute() {
        bail!("home_dir must be absolute: {}", home_dir.display());
    }
    if !cwd.is_absolute() {
        bail!("cwd must be absolute: {}", cwd.display());
    }

    let home_dir = normalize_lexical(home_dir);
    let cwd = normalize_lexical(cwd);
    let out_dir = match out_dir_override {
        Some(path) => resolve_user_path(path, &home_dir, &cwd)?,
        None => home_dir.join(".logit").join("output"),
    };

    Ok(RuntimePaths {
        home_dir,
        cwd,
        out_dir: normalize_lexical(&out_dir),
    })
}

fn resolve_user_path(path: &Path, home_dir: &Path, cwd: &Path) -> Result<PathBuf> {
    let expanded = expand_tilde(path, home_dir)?;
    let resolved = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };

    Ok(normalize_lexical(&resolved))
}

fn expand_tilde(path: &Path, home_dir: &Path) -> Result<PathBuf> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(first)) if first == "~" => {
            let mut expanded = home_dir.to_path_buf();
            for component in components {
                expanded.push(component.as_os_str());
            }
            Ok(expanded)
        }
        Some(Component::Normal(first))
            if first
                .to_str()
                .is_some_and(|segment| segment.starts_with('~')) =>
        {
            bail!(
                "unsupported home expansion syntax (only `~` and `~/...` are supported): {}",
                path.display()
            )
        }
        _ => Ok(path.to_path_buf()),
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::resolve_runtime_paths;
    use std::path::Path;

    #[test]
    fn defaults_out_dir_under_logit_output() {
        let paths = resolve_runtime_paths(Path::new("/home/tester"), Path::new("/work/repo"), None)
            .expect("paths should resolve");

        assert_eq!(paths.home_dir, Path::new("/home/tester"));
        assert_eq!(paths.cwd, Path::new("/work/repo"));
        assert_eq!(paths.out_dir, Path::new("/home/tester/.logit/output"));
    }

    #[test]
    fn expands_tilde_override_against_home_dir() {
        let paths = resolve_runtime_paths(
            Path::new("/home/tester"),
            Path::new("/work/repo"),
            Some(Path::new("~/custom/output")),
        )
        .expect("tilde override should resolve");

        assert_eq!(paths.out_dir, Path::new("/home/tester/custom/output"));
    }

    #[test]
    fn resolves_relative_override_against_cwd() {
        let paths = resolve_runtime_paths(
            Path::new("/home/tester"),
            Path::new("/work/repo"),
            Some(Path::new("./artifacts/../artifacts/runs")),
        )
        .expect("relative override should resolve");

        assert_eq!(paths.out_dir, Path::new("/work/repo/artifacts/runs"));
    }

    #[test]
    fn rejects_non_absolute_home_dir() {
        let err = resolve_runtime_paths(Path::new("home/tester"), Path::new("/work/repo"), None)
            .expect_err("relative home dir must fail");

        assert!(
            err.to_string().contains("home_dir must be absolute"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_tilde_username_syntax() {
        let err = resolve_runtime_paths(
            Path::new("/home/tester"),
            Path::new("/work/repo"),
            Some(Path::new("~someone/out")),
        )
        .expect_err("~username syntax must fail");

        assert!(
            err.to_string()
                .contains("unsupported home expansion syntax"),
            "unexpected error: {err}"
        );
    }
}
