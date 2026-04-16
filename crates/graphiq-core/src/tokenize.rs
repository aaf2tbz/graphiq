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
