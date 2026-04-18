pub fn decompose_identifier(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }

    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c == '_' || c.is_whitespace() || c == '.' || c == ':' || c == '$' || c == '-' {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            i += 1;
            continue;
        }

        if c.is_ascii_digit() {
            current.push(c);
            i += 1;
            continue;
        }

        if c.is_ascii_uppercase() {
            if !current.is_empty() {
                let all_upper_so_far = current
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit());
                if !all_upper_so_far {
                    tokens.push(current.clone());
                    current.clear();
                    current.push(c);
                    i += 1;
                    continue;
                }
            }
            current.push(c);
            i += 1;

            while i < chars.len() {
                let nc = chars[i];
                if nc.is_ascii_uppercase() {
                    current.push(nc);
                    i += 1;
                } else if nc.is_ascii_digit() {
                    current.push(nc);
                    i += 1;
                } else {
                    break;
                }
            }

            if i < chars.len() && chars[i].is_ascii_lowercase() {
                if current.len() > 1 {
                    let last = current.pop().unwrap();
                    tokens.push(current.clone());
                    current.clear();
                    current.push(last);
                }
            }
            continue;
        }

        current.push(c);
        i += 1;
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
        .iter()
        .map(|t| t.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn stem_word(word: &str) -> String {
    let w = word.to_lowercase();
    if w.len() < 4 {
        return w;
    }

    let mut s = w.clone();

    if s.ends_with("ies") && s.len() > 4 {
        s = s[..s.len() - 3].to_string() + "i";
    } else if s.ends_with("es") && s.len() > 4 {
        let stem = &s[..s.len() - 2];
        if stem.ends_with("ss")
            || stem.ends_with("sh")
            || stem.ends_with("ch")
            || stem.ends_with("x")
            || stem.ends_with("z")
        {
            s = stem.to_string() + "e";
        } else {
            s = stem.to_string();
        }
    } else if s.ends_with("ed") && s.len() > 4 {
        let stem = &s[..s.len() - 2];
        if stem.ends_with("e") {
            s = stem.to_string();
        } else {
            s = stem.to_string();
        }
    } else if s.ends_with("ing") && s.len() > 5 {
        let stem = &s[..s.len() - 3];
        if stem.ends_with("e") {
            s = stem.to_string();
        } else if stem.len() >= 3 {
            s = stem.to_string();
        }
    } else if s.ends_with("tion") {
        s = s[..s.len() - 4].to_string() + "t";
    } else if s.ends_with("sion") {
        s = s[..s.len() - 4].to_string() + "s";
    } else if s.ends_with("ment") {
        s = s[..s.len() - 4].to_string();
    } else if s.ends_with("ness") {
        s = s[..s.len() - 4].to_string();
    } else if s.ends_with("able") || s.ends_with("ible") {
        s = s[..s.len() - 4].to_string();
    } else if s.ends_with("ful") {
        s = s[..s.len() - 3].to_string();
    } else if s.ends_with("less") {
        s = s[..s.len() - 4].to_string();
    } else if s.ends_with("ous") {
        s = s[..s.len() - 3].to_string();
    } else if s.ends_with("ive") {
        s = s[..s.len() - 3].to_string();
    } else if s.ends_with("er") && s.len() > 4 {
        s = s[..s.len() - 2].to_string();
    } else if s.ends_with("ly") && s.len() > 4 {
        s = s[..s.len() - 2].to_string();
    } else if s.ends_with("al") && s.len() > 4 {
        s = s[..s.len() - 2].to_string();
    }

    if s.ends_with("at") || s.ends_with("bl") || s.ends_with("iz") {
        s.push('e');
    }

    if s.len() >= 3 && s.ends_with("y") && !s.ends_with("ay") && !s.ends_with("ey") {
        s = s[..s.len() - 1].to_string() + "i";
    }

    if s.is_empty() {
        w
    } else {
        s
    }
}

pub fn stem_text(text: &str) -> String {
    text.split_whitespace()
        .filter(|w| w.len() >= 2)
        .map(|w| stem_word(w))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_case() {
        assert_eq!(
            decompose_identifier("authenticateUser"),
            "authenticate user"
        );
        assert_eq!(decompose_identifier("getUserName"), "get user name");
        assert_eq!(decompose_identifier("HTTPClient"), "http client");
    }

    #[test]
    fn test_snake_case() {
        assert_eq!(
            decompose_identifier("rate_limit_middleware"),
            "rate limit middleware"
        );
        assert_eq!(decompose_identifier("_private_field"), "private field");
        assert_eq!(decompose_identifier("__dunder__"), "dunder");
    }

    #[test]
    fn test_screaming_snake() {
        assert_eq!(
            decompose_identifier("RATE_LIMIT_CONFIG"),
            "rate limit config"
        );
        assert_eq!(decompose_identifier("MAX_RETRIES"), "max retries");
    }

    #[test]
    fn test_pascal_case() {
        assert_eq!(decompose_identifier("RateLimiter"), "rate limiter");
        assert_eq!(decompose_identifier("XMLParser"), "xml parser");
        assert_eq!(decompose_identifier("HTMLElement"), "html element");
    }

    #[test]
    fn test_all_caps_acronym() {
        assert_eq!(decompose_identifier("API"), "api");
        assert_eq!(decompose_identifier("HTTP"), "http");
        assert_eq!(decompose_identifier("URL"), "url");
    }

    #[test]
    fn test_single_word() {
        assert_eq!(decompose_identifier("main"), "main");
        assert_eq!(decompose_identifier("Main"), "main");
        assert_eq!(decompose_identifier("MAIN"), "main");
    }

    #[test]
    fn test_dotted_path() {
        assert_eq!(
            decompose_identifier("auth.RateLimiter"),
            "auth rate limiter"
        );
        assert_eq!(
            decompose_identifier("std.collections.HashMap"),
            "std collections hash map"
        );
    }

    #[test]
    fn test_edge_cases() {
        assert_eq!(decompose_identifier(""), "");
        assert_eq!(decompose_identifier("_"), "");
        assert_eq!(decompose_identifier("__"), "");
        assert_eq!(decompose_identifier("a"), "a");
        assert_eq!(decompose_identifier("A"), "a");
    }

    #[test]
    fn test_mixed_separators() {
        assert_eq!(
            decompose_identifier("getHTTPResponse_code"),
            "get http response code"
        );
        assert_eq!(decompose_identifier("XML_Parser"), "xml parser");
    }

    #[test]
    fn test_identifier_with_numbers() {
        assert_eq!(decompose_identifier("sha256Hash"), "sha256 hash");
        assert_eq!(
            decompose_identifier("parseHTTP2Request"),
            "parse http2 request"
        );
    }

    #[test]
    fn test_dollar_sign() {
        assert_eq!(decompose_identifier("jQuery$element"), "j query element");
    }
}
