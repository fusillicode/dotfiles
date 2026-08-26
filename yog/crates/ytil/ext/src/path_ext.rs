//! Extensions for [`Path`] and [`std::path::PathBuf`]

use std::path::Path;

pub trait PathExt {
    fn short_path(&self, home: &Self) -> String;
}

impl PathExt for Path {
    fn short_path(&self, home: &Self) -> String {
        if home != Self::new("/") {
            if self == home {
                return "~".into();
            }
            if let Ok(rel) = self.strip_prefix(home) {
                let names = path_dir_names(rel);
                return if names.is_empty() {
                    "~".into()
                } else {
                    format!("~/{}", abbrev_path_dirs(&names))
                };
            }
        }

        let names = path_dir_names(self);
        if names.is_empty() {
            "/".into()
        } else {
            format!("/{}", abbrev_path_dirs(&names))
        }
    }
}

fn path_dir_names(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir
            | std::path::Component::ParentDir => None,
        })
        .collect()
}

fn abbrev_path_dirs(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names.first().cloned().unwrap_or_default(),
        total => {
            let mut out = String::new();
            for (idx, name) in names.iter().enumerate() {
                if idx > 0 {
                    out.push('/');
                }
                let is_last = idx == total.saturating_sub(1);
                if is_last {
                    out.push_str(name);
                } else {
                    out.push(name.chars().next().unwrap_or('·'));
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_short_path_under_home_abbreviates_parent_directories() {
        let home = Path::new("/home/user");

        assert_that!(
            Path::new("/home/user/src/pkg/myproject").short_path(home),
            eq("~/s/p/myproject")
        );
    }

    #[test]
    fn test_short_path_many_dirs_abbreviates_all_but_last() {
        let home = Path::new("/home/user");

        assert_that!(
            Path::new("/home/user/one/two/three/four/five").short_path(home),
            eq("~/o/t/t/f/five")
        );
    }

    #[test]
    fn test_short_path_outside_home_renders_absolute_abbrev() {
        let home = Path::new("/home/user");

        assert_that!(Path::new("/opt/pkg/foo").short_path(home), eq("/o/p/foo"));
    }
}
