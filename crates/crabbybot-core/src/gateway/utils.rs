//! Shared gateway utilities.

/// Split a message into chunks of at most `max_len` characters,
/// preferring to break at newlines when possible.
///
/// Used by both the Telegram and Discord transports to respect
/// platform-specific message length limits.
pub fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_owned()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_owned());
            break;
        }

        // Try to find a newline to break at
        let slice = &remaining[..max_len];
        let break_at = slice.rfind('\n').unwrap_or(max_len);
        let break_at = if break_at == 0 { max_len } else { break_at };

        chunks.push(remaining[..break_at].to_owned());
        remaining = &remaining[break_at..].trim_start_matches('\n');
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_short_message() {
        let chunks = chunk_message("hello", 4096);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_chunk_long_message() {
        let long = "a".repeat(5000);
        let chunks = chunk_message(&long, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 4096);
        assert_eq!(chunks[1].len(), 904);
    }

    #[test]
    fn test_chunk_at_newline() {
        let text = format!("{}\n{}", "a".repeat(100), "b".repeat(100));
        let chunks = chunk_message(&text, 150);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "a".repeat(100));
        assert_eq!(chunks[1], "b".repeat(100));
    }

    #[test]
    fn test_chunk_discord_limit() {
        let long = "a".repeat(3000);
        let chunks = chunk_message(&long, 2000);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2000);
        assert_eq!(chunks[1].len(), 1000);
    }
}
