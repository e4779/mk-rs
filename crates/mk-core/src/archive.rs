//! Archive member syntax: `lib.a(member.o)`.
//!
//! Plan 9 mk supports the `archive(member)` syntax where a target like
//! `lib.a(foo.o)` means that `foo.o` is a member of the archive `lib.a`.
//! The archive itself is updated via a separate `ar` recipe.

/// Parsed archive member reference: `lib.a(member.o)`
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveRef {
    pub archive: String,
    pub member: String,
}

/// Parse "archive(member)" syntax.
/// Returns `None` if the name doesn't match the pattern.
pub fn parse_archive_ref(name: &str) -> Option<ArchiveRef> {
    if let Some(open) = name.find('(') {
        if name.ends_with(')') {
            let archive = name[..open].to_string();
            let member = name[open + 1..name.len() - 1].to_string();
            if !archive.is_empty() && !member.is_empty() {
                return Some(ArchiveRef { archive, member });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let r = parse_archive_ref("lib.a(foo.o)").unwrap();
        assert_eq!(r.archive, "lib.a");
        assert_eq!(r.member, "foo.o");
    }

    #[test]
    fn parse_no_parens() {
        assert!(parse_archive_ref("foo.o").is_none());
    }

    #[test]
    fn parse_empty_archive() {
        assert!(parse_archive_ref("(foo.o)").is_none());
    }

    #[test]
    fn parse_empty_member() {
        assert!(parse_archive_ref("lib.a()").is_none());
    }

    #[test]
    fn parse_slash_in_archive() {
        let r = parse_archive_ref("dir/lib.a(foo.o)").unwrap();
        assert_eq!(r.archive, "dir/lib.a");
        assert_eq!(r.member, "foo.o");
    }

    #[test]
    fn parse_slash_in_member() {
        let r = parse_archive_ref("lib.a(sub/foo.o)").unwrap();
        assert_eq!(r.archive, "lib.a");
        assert_eq!(r.member, "sub/foo.o");
    }

    #[test]
    fn parse_no_close_paren() {
        assert!(parse_archive_ref("lib.a(foo.o").is_none());
    }
}
