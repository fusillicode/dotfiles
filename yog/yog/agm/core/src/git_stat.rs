use std::path::PathBuf;

use crate::ParseError;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitStat {
    pub insertions: usize,
    pub deletions: usize,
    pub new_files: usize,
    pub is_worktree: bool,
}

impl GitStat {
    pub fn parse_line(line: &str) -> Result<(PathBuf, Self), ParseError> {
        let mut parts = line.rsplitn(5, ' ');
        let is_worktree = parts
            .next()
            .ok_or(ParseError::Missing("worktree field"))
            .and_then(|v| {
                v.parse::<u8>().map_err(|_| ParseError::Invalid {
                    field: "worktree",
                    value: format!("{v:?}"),
                })
            })?
            != 0;
        let new_files = parts
            .next()
            .ok_or(ParseError::Missing("new_files field"))
            .and_then(|v| {
                v.parse().map_err(|_| ParseError::Invalid {
                    field: "new_files",
                    value: format!("{v:?}"),
                })
            })?;
        let deletions = parts
            .next()
            .ok_or(ParseError::Missing("deletions field"))
            .and_then(|v| {
                v.parse().map_err(|_| ParseError::Invalid {
                    field: "deletions",
                    value: format!("{v:?}"),
                })
            })?;
        let insertions = parts
            .next()
            .ok_or(ParseError::Missing("insertions field"))
            .and_then(|v| {
                v.parse().map_err(|_| ParseError::Invalid {
                    field: "insertions",
                    value: format!("{v:?}"),
                })
            })?;
        let path = PathBuf::from(parts.next().ok_or(ParseError::Missing("path"))?);
        Ok((
            path,
            Self {
                insertions,
                deletions,
                new_files,
                is_worktree,
            },
        ))
    }
}

impl std::fmt::Display for GitStat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.insertions,
            self.deletions,
            self.new_files,
            u8::from(self.is_worktree),
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("/home/user/project 10 5 2 1", "/home/user/project", 10, 5, 2, true)]
    #[case("/home/user/my project 10 5 2 0", "/home/user/my project", 10, 5, 2, false)]
    fn git_stat_parse_line_works_as_expected(
        #[case] line: &str,
        #[case] expected_path: &str,
        #[case] insertions: usize,
        #[case] deletions: usize,
        #[case] new_files: usize,
        #[case] is_worktree: bool,
    ) {
        let expected_stat = GitStat {
            insertions,
            deletions,
            new_files,
            is_worktree,
        };
        assert2::assert!(let Ok((path, stat)) = GitStat::parse_line(line));
        pretty_assertions::assert_eq!((path, stat), (PathBuf::from(expected_path), expected_stat));
    }
}
