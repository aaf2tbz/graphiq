pub struct RateLimiter {
    max_requests: u64,
    window_ms: u64,
    current_count: u64,
    window_start: std::time::Instant,
}

impl RateLimiter {
    pub fn new(max_requests: u64, window_ms: u64) -> Self {
        Self {
            max_requests,
            window_ms,
            current_count: 0,
            window_start: std::time::Instant::now(),
        }
    }

    pub fn check_rate_limit(&mut self) -> bool {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.window_start);
        if elapsed.as_millis() as u64 >= self.window_ms {
            self.window_start = now;
            self.current_count = 0;
        }
        if self.current_count < self.max_requests {
            self.current_count += 1;
            true
        } else {
            false
        }
    }

    pub fn remaining_quota(&self) -> u64 {
        self.max_requests.saturating_sub(self.current_count)
    }

    pub fn reset_window(&mut self) {
        self.current_count = 0;
        self.window_start = std::time::Instant::now();
    }
}

pub fn validate_auth_token(token: &str) -> Result<String, String> {
    if token.is_empty() {
        return Err("empty token".into());
    }
    if token.len() < 16 {
        return Err("token too short".into());
    }
    Ok(format!("user_{}", &token[..8]))
}

pub fn encode_base64(input: &[u8]) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARSET[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARSET[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARSET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARSET[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

pub fn compute_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn sanitize_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
