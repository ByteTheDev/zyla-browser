//! Turns what the user types in the address bar into a URL.
//!
//! Only web and local-file schemes are accepted from typed input. Anything that does not look like
//! an address becomes a search query, so `javascript:` and `data:` URLs cannot be typed in.

const SEARCH_URL: &str = "https://www.google.com/search?q=";

/// Returns the URL to load for `input`, or `None` when the input is empty.
pub fn resolve(input: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(url) = explicit_scheme(text) {
        return Some(url);
    }
    if !text.contains(char::is_whitespace) && looks_like_host(text) {
        let scheme = if is_local_host(text) { "http" } else { "https" };
        return Some(format!("{scheme}://{text}"));
    }
    Some(format!("{SEARCH_URL}{}", percent_encode(text)))
}

fn explicit_scheme(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    ["https://", "http://", "file://", "about:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
        .then(|| text.to_string())
}

fn is_local_host(text: &str) -> bool {
    let host = text.split(['/', '?', '#']).next().unwrap_or("");
    let name = host.split(':').next().unwrap_or("");
    matches!(name, "localhost" | "127.0.0.1" | "[::1]")
}

fn looks_like_host(text: &str) -> bool {
    let host = text.split(['/', '?', '#']).next().unwrap_or("");
    let name = host.split(':').next().unwrap_or("");
    if name.is_empty() || name.starts_with('.') || name.ends_with('.') {
        return false;
    }
    if is_local_host(text) {
        return true;
    }
    // Only letters, digits, dots and hyphens in the host name, and a dot to separate it from a
    // search term. A bare word such as "weather" goes to search.
    let plausible = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    plausible && name.contains('.')
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn empty_input_resolves_to_nothing() {
        assert_eq!(resolve("   "), None);
    }

    #[test]
    fn explicit_web_urls_are_kept() {
        assert_eq!(resolve("https://example.com/a?b=1").as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(resolve("HTTP://Example.com").as_deref(), Some("HTTP://Example.com"));
    }

    #[test]
    fn hosts_get_a_scheme() {
        assert_eq!(resolve("example.com/docs").as_deref(), Some("https://example.com/docs"));
        assert_eq!(resolve("localhost:3000").as_deref(), Some("http://localhost:3000"));
    }

    #[test]
    fn everything_else_is_a_search() {
        assert_eq!(resolve("rust gpui").as_deref(), Some("https://www.google.com/search?q=rust%20gpui"));
        assert_eq!(resolve("weather").as_deref(), Some("https://www.google.com/search?q=weather"));
    }

    #[test]
    fn script_and_data_urls_are_searched_not_loaded() {
        assert!(resolve("javascript:alert(1)").unwrap().starts_with("https://www.google.com/search?q="));
        assert!(resolve("data:text/html,hi").unwrap().starts_with("https://www.google.com/search?q="));
    }
}
