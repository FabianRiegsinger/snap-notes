//! Enter inside a Markdown list item: the new line gets the next marker, and
//! Enter on an empty item ends the list.

/// What Enter does on a list line instead of a plain line break: delete
/// `delete` characters before the cursor, then insert `insert` at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEdit {
    pub delete: usize,
    pub insert: String,
}

/// The list edit for Enter at byte `cursor_col` of `line`; `None` when
/// `line` is no list item or the cursor sits before its text.
pub fn continue_list(line: &str, cursor_col: usize) -> Option<ListEdit> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let rest = &line[indent..];
    let (marker_len, next) = match rest.as_bytes().first()? {
        b'-' | b'*' | b'+' => (1, rest[..1].to_string()),
        b'0'..=b'9' => {
            // CommonMark caps ordered markers at nine digits.
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            let delim = rest[digits..]
                .chars()
                .next()
                .filter(|c| matches!(c, '.' | ')'))?;
            let n: u64 = rest[..digits].parse().ok().filter(|_| digits <= 9)?;
            (digits + 1, format!("{}{delim}", n + 1))
        }
        _ => return None,
    };
    let after = &rest[marker_len..];
    let body = after.trim_start_matches([' ', '\t']);
    if body.len() == after.len() || cursor_col <= indent + marker_len || cursor_col > line.len() {
        return None;
    }
    let task = body
        .strip_prefix("[ ]")
        .or_else(|| body.strip_prefix("[x]"))
        .or_else(|| body.strip_prefix("[X]"))
        .filter(|t| t.is_empty() || t.starts_with([' ', '\t']));
    let text = task.unwrap_or(body);
    // Only spaces and tabs: other whitespace (a no-break space) is text.
    if text.trim_matches([' ', '\t']).is_empty() && cursor_col == line.len() {
        return Some(ListEdit {
            delete: line.chars().count(),
            insert: String::new(),
        });
    }
    let check = if task.is_some() { "[ ] " } else { "" };
    Some(ListEdit {
        delete: 0,
        insert: format!("\n{}{next} {check}", &line[..indent]),
    })
}

/// [`continue_list`] for 0-based `line` of `content`, except inside fenced
/// code, where Enter stays a plain line break.
pub fn continue_list_at(content: &str, line: usize, cursor_col: usize) -> Option<ListEdit> {
    // The open fence's character and length, if inside one.
    let mut fence: Option<(u8, usize)> = None;
    for (i, text) in content.split('\n').enumerate() {
        let trimmed = text.trim_start_matches(' ');
        let run = |c: u8| trimmed.bytes().take_while(|&b| b == c).count();
        let in_fence = fence.is_some();
        match fence {
            Some((c, len)) => {
                if run(c) >= len && trimmed[run(c)..].trim().is_empty() {
                    fence = None;
                }
            }
            None => {
                fence = b"`~"
                    .iter()
                    .map(|&c| (c, run(c)))
                    .find(|&(_, len)| len >= 3);
            }
        }
        if i == line {
            return (!in_fence && fence.is_none())
                .then(|| continue_list(text, cursor_col))
                .flatten();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cont(insert: &str) -> Option<ListEdit> {
        Some(ListEdit {
            delete: 0,
            insert: insert.into(),
        })
    }

    fn end(delete: usize) -> Option<ListEdit> {
        Some(ListEdit {
            delete,
            insert: String::new(),
        })
    }

    #[test]
    fn ordered_items_count_up_and_keep_the_delimiter() {
        assert_eq!(continue_list("1. a", 4), cont("\n2. "));
        assert_eq!(continue_list("3) a", 4), cont("\n4) "));
        assert_eq!(continue_list("9. nine", 7), cont("\n10. "));
        assert_eq!(continue_list("99) x", 5), cont("\n100) "));
    }

    #[test]
    fn unordered_items_repeat_their_marker() {
        assert_eq!(continue_list("- a", 3), cont("\n- "));
        assert_eq!(continue_list("* a", 3), cont("\n* "));
        assert_eq!(continue_list("+ a", 3), cont("\n+ "));
    }

    #[test]
    fn task_items_continue_unchecked() {
        assert_eq!(continue_list("- [ ] a", 7), cont("\n- [ ] "));
        assert_eq!(continue_list("- [x] done", 10), cont("\n- [ ] "));
        assert_eq!(continue_list("* [X] done", 10), cont("\n* [ ] "));
        assert_eq!(continue_list("1. [x] a", 8), cont("\n2. [ ] "));
    }

    #[test]
    fn indentation_is_kept() {
        assert_eq!(continue_list("  - a", 5), cont("\n  - "));
        assert_eq!(continue_list("\t1. a", 5), cont("\n\t2. "));
        assert_eq!(continue_list("    - [ ] a", 11), cont("\n    - [ ] "));
    }

    #[test]
    fn empty_item_ends_the_list() {
        assert_eq!(continue_list("- ", 2), end(2));
        assert_eq!(continue_list("2. ", 3), end(3));
        assert_eq!(continue_list("  - [ ] ", 8), end(8));
        assert_eq!(continue_list("- [x] ", 6), end(6));
        assert_eq!(continue_list("-   ", 4), end(4));
    }

    #[test]
    fn unicode_space_is_text_not_an_empty_item() {
        // A no-break space is content: the item continues, nothing is deleted.
        let line = "- \u{a0}";
        assert_eq!(continue_list(line, line.len()), cont("\n- "));
    }

    #[test]
    fn enter_mid_text_splits_the_item() {
        assert_eq!(continue_list("1. abcd", 5), cont("\n2. "));
        assert_eq!(continue_list("- [x] abcd", 7), cont("\n- [ ] "));
        // Only the marker before the cursor: an item with text after it.
        assert_eq!(continue_list("- abc", 2), cont("\n- "));
    }

    #[test]
    fn non_list_lines_are_left_alone() {
        assert_eq!(continue_list("", 0), None);
        assert_eq!(continue_list("plain text", 10), None);
        assert_eq!(continue_list("-no space", 9), None);
        assert_eq!(continue_list("1.no space", 10), None);
        assert_eq!(continue_list("1.", 2), None);
        assert_eq!(continue_list("-", 1), None);
        assert_eq!(continue_list("a. b", 4), None);
        assert_eq!(continue_list("# heading", 9), None);
        assert_eq!(continue_list("---", 3), None);
    }

    #[test]
    fn cursor_inside_the_marker_is_a_plain_break() {
        assert_eq!(continue_list("- a", 0), None);
        assert_eq!(continue_list("- a", 1), None);
        assert_eq!(continue_list("12. a", 2), None);
        assert_eq!(continue_list("  - a", 1), None);
    }

    #[test]
    fn huge_numbers_do_not_overflow() {
        assert_eq!(continue_list("99999999999999999999999. a", 26), None);
    }

    #[test]
    fn fenced_code_is_ignored() {
        let text = "- a\n```\n- b\n```\n- c";
        assert_eq!(continue_list_at(text, 0, 3), cont("\n- "));
        assert_eq!(continue_list_at(text, 2, 3), None);
        assert_eq!(continue_list_at(text, 4, 3), cont("\n- "));
        let tilde = "~~~\n1. x\n```\n2. y\n~~~\n3. z";
        assert_eq!(continue_list_at(tilde, 1, 4), None);
        assert_eq!(continue_list_at(tilde, 3, 4), None);
        assert_eq!(continue_list_at(tilde, 5, 4), cont("\n4. "));
        // An unclosed fence runs to the end.
        assert_eq!(continue_list_at("```\n- a", 1, 3), None);
        // Out of range lines are no list items.
        assert_eq!(continue_list_at("- a", 5, 0), None);
    }
}
