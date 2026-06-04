pub fn mask_line(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    let mask = |out: &mut Vec<u8>, j: usize| {
        if out[j].is_ascii() {
            out[j] = b' ';
        }
    };
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' || c == b'`' {
            let quote = c;
            mask(&mut out, i);
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' && i + 1 < b.len() {
                    mask(&mut out, i);
                    mask(&mut out, i + 1);
                    i += 2;
                    continue;
                }
                let end = b[i] == quote;
                mask(&mut out, i);
                i += 1;
                if end {
                    break;
                }
            }
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            for j in i..b.len() {
                mask(&mut out, j);
            }
            break;
        } else if c == b'#' {
            for j in i..b.len() {
                mask(&mut out, j);
            }
            break;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            mask(&mut out, i);
            mask(&mut out, i + 1);
            i += 2;
            while i < b.len() {
                if b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'/' {
                    mask(&mut out, i);
                    mask(&mut out, i + 1);
                    i += 2;
                    break;
                }
                mask(&mut out, i);
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_untouched() {
        assert_eq!(mask_line("if a == b {"), "if a == b {");
    }

    #[test]
    fn test_double_quote_string_masked() {
        let m = mask_line(r#"let s = "a == b";"#);
        assert!(!m.contains("=="));
        assert_eq!(m.len(), r#"let s = "a == b";"#.len());
        assert!(m.starts_with("let s = "));
    }

    #[test]
    fn test_line_comment_masked() {
        let m = mask_line("x = 1 // y == z");
        assert!(m.starts_with("x = 1 "));
        assert!(!m.contains("=="));
    }

    #[test]
    fn test_hash_comment_masked() {
        let m = mask_line("x = 1  # y == z");
        assert!(!m.contains("=="));
    }

    #[test]
    fn test_block_comment_masked() {
        let m = mask_line("a /* == */ == b");
        assert_eq!(m.matches("==").count(), 1);
        assert_eq!(m.len(), "a /* == */ == b".len());
    }

    #[test]
    fn test_escaped_quote_inside_string() {
        let m = mask_line(r#""he said \"==\" ok" == x"#);
        assert_eq!(m.matches("==").count(), 1);
    }

    #[test]
    fn test_multibyte_inside_string_preserved_length() {
        let src = r#"let s = "café == x";"#;
        let m = mask_line(src);
        assert_eq!(m.len(), src.len());
        assert!(!m.contains("=="));
    }
}
