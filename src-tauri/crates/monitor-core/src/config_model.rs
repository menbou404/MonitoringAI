use std::ops::Range;

// Track strings and containers so table-like text inside other values is never a header.
fn model_value(config: &str) -> Result<Option<Range<usize>>, &'static str> {
    let mut quote = None;
    let mut multiline = false;
    let mut depth = 0_u32;
    let mut offset = 0;
    for line in config.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if quote.is_none() && depth == 0 {
            if trimmed.starts_with('[') {
                break;
            }
            let key_end = if trimmed.starts_with("\"model\"") || trimmed.starts_with("'model'") {
                Some(7)
            } else if trimmed.starts_with("model") {
                Some(5)
            } else {
                None
            };
            if let Some(end) = key_end {
                let rest = trimmed[end..].trim_start();
                if let Some(value) = rest.strip_prefix('=') {
                    let value = value.trim_start();
                    let start = offset + line.len() - value.len();
                    let delimiter = value
                        .as_bytes()
                        .first()
                        .copied()
                        .ok_or("model value is missing")?;
                    if !matches!(delimiter, b'"' | b'\'')
                        || value.starts_with("\"\"\"")
                        || value.starts_with("'''")
                    {
                        return Err("unsupported model value");
                    }
                    let mut escaped = false;
                    for (i, byte) in value.bytes().enumerate().skip(1) {
                        if !escaped && byte == delimiter {
                            if !value[i + 1..].trim().is_empty()
                                && !value[i + 1..].trim_start().starts_with('#')
                            {
                                return Err("invalid model value");
                            }
                            return Ok(Some(start..start + i + 1));
                        }
                        escaped = delimiter == b'"' && !escaped && byte == b'\\';
                    }
                    return Err("unterminated model value");
                }
            }
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if let Some(q) = quote {
                if q == b'"' && b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q && (!multiline || bytes.get(i..i + 3) == Some(&[q, q, q])) {
                    quote = None;
                    i += if multiline {
                        bytes[i..].iter().take_while(|b| **b == q).count()
                    } else {
                        1
                    };
                    continue;
                }
            } else {
                match b {
                    b'#' => break,
                    b'"' | b'\'' => {
                        quote = Some(b);
                        multiline = bytes.get(i..i + 3) == Some(&[b, b, b]);
                        if multiline {
                            i += 3;
                            continue;
                        }
                    }
                    b'[' | b'{' => depth += 1,
                    b']' | b'}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            i += 1;
        }
        offset += line.len();
    }
    Ok(None)
}

pub fn config_model(config: &str) -> Result<Option<String>, &'static str> {
    model_value(config)?
        .map(|range| {
            let value = &config[range];
            if value.starts_with('\'') {
                Ok(value[1..value.len() - 1].to_owned())
            } else {
                serde_json::from_str(value).map_err(|_| "unsupported model string")
            }
        })
        .transpose()
}

pub fn replace_config_model(config: &str, model: &str) -> Result<String, &'static str> {
    if model.trim().is_empty() || model.chars().any(char::is_control) {
        return Err("invalid model name");
    }
    let value = serde_json::to_string(model).map_err(|_| "invalid model name")?;
    if let Some(range) = model_value(config)? {
        let mut result = config.to_owned();
        result.replace_range(range, &value);
        Ok(result)
    } else {
        let newline = if config.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        Ok(format!("model = {value}{newline}{config}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_root_value_and_preserves_comments_and_endings() {
        for newline in ["\n", "\r\n"] {
            let input = [
                "# keep",
                " model = 'base'  # default",
                "[profiles.x]",
                "model = \"profile\"",
            ]
            .join(newline);
            let expected = input.replace("'base'", "\"saver\"");
            assert_eq!(replace_config_model(&input, "saver").unwrap(), expected);
            assert_eq!(config_model(&expected).unwrap().as_deref(), Some("saver"));
        }
    }

    #[test]
    fn inserts_before_tables_and_ignores_model_like_keys() {
        for input in [
            "",
            "# note\r\n[profiles.x]\r\nmodel = \"profile\"\r\n",
            "model_provider = \"other\"\n",
        ] {
            let newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
            assert_eq!(config_model(input).unwrap(), None);
            assert_eq!(
                replace_config_model(input, "base").unwrap(),
                format!("model = \"base\"{newline}{input}")
            );
        }
    }

    #[test]
    fn ignores_multiline_strings_arrays_and_inline_tables() {
        let input = "note = '''\n[not.a.table]\nmodel = \"text\"\n'''\nitems = [\n'''\nmodel = \"text\"\n'''\n]\nmodel = \"base\"\n[profiles.x]\nmodel = \"profile\"\n";
        assert_eq!(
            replace_config_model(input, "saver").unwrap(),
            input.replace("model = \"base\"", "model = \"saver\"")
        );
        for quotes in ["''''", "'''''", "\"\"\"\"", "\"\"\"\"\""] {
            let delimiter = &quotes[..3];
            let input = format!("note = {delimiter}text{quotes}\nmodel = \"base\"\n");
            assert_eq!(config_model(&input).unwrap().as_deref(), Some("base"));
        }
        let input = "other = { model = \"inline\" }\nmodel = \"base\"\n";
        assert_eq!(
            replace_config_model(input, "saver").unwrap(),
            input.replace("\"base\"", "\"saver\"")
        );
    }

    #[test]
    fn escapes_names_and_rejects_unsafe_values() {
        let changed = replace_config_model("\"model\"=\"old\" # keep\n", "a\"b\\c").unwrap();
        assert_eq!(config_model(&changed).unwrap().as_deref(), Some("a\"b\\c"));
        for input in ["model = 1", "model = \"broken", "model = \"x\" extra"] {
            assert!(replace_config_model(input, "new").is_err());
        }
        assert!(replace_config_model("", "bad\nname").is_err());
    }
}
