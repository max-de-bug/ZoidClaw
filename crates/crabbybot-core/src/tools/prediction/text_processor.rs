//! Text chunking utility.
//!
//! Splits large documents into overlapping chunks for LLM processing,
//! mirroring MiroFish's `TextProcessor.split_text()`.

/// Split `text` into chunks of approximately `chunk_size` characters
/// with `overlap` characters of overlap between consecutive chunks.
///
/// Chunks are split on sentence boundaries when possible to avoid
/// cutting mid-sentence.
pub fn split_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    if text.is_empty() || chunk_size == 0 {
        return Vec::new();
    }

    let text = text.trim();
    if text.len() <= chunk_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let mut start = 0;

    while start < total {
        let end = (start + chunk_size).min(total);

        // Try to find a sentence boundary near the end of the chunk
        let boundary = find_sentence_boundary(&chars, start, end);
        let chunk_end = boundary.unwrap_or(end);

        let chunk: String = chars[start..chunk_end].iter().collect();
        let trimmed = chunk.trim();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }

        // Advance by (chunk_end - start - overlap), ensuring forward progress
        let advance = (chunk_end - start).saturating_sub(overlap).max(1);
        start += advance;
    }

    chunks
}

/// Look backwards from `end` for a sentence-ending character (`. ! ?`)
/// within the last 20% of the chunk. Returns the position *after* the
/// sentence ender so the chunk includes the punctuation.
fn find_sentence_boundary(chars: &[char], start: usize, end: usize) -> Option<usize> {
    let search_start = start + (end - start) * 80 / 100; // last 20%
    for i in (search_start..end).rev() {
        if matches!(chars[i], '.' | '!' | '?') {
            // Include the punctuation and any trailing whitespace
            let boundary = (i + 1).min(chars.len());
            return Some(boundary);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input() {
        assert!(split_text("", 100, 10).is_empty());
    }

    #[test]
    fn short_text_single_chunk() {
        let chunks = split_text("Hello world.", 100, 10);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world.");
    }

    #[test]
    fn splits_on_size() {
        let text = "a".repeat(250);
        let chunks = split_text(&text, 100, 0);
        assert!(chunks.len() >= 3);
        // All chunks should be non-empty
        assert!(chunks.iter().all(|c| !c.is_empty()));
    }

    #[test]
    fn overlap_creates_more_chunks() {
        let text = "a".repeat(200);
        let no_overlap = split_text(&text, 100, 0);
        let with_overlap = split_text(&text, 100, 30);
        assert!(with_overlap.len() >= no_overlap.len());
    }

    #[test]
    fn prefers_sentence_boundaries() {
        let text = "First sent. Second sent. Third sent. Fourth sent. Fifth sent. Sixth sent.";
        let chunks = split_text(text, 40, 5);
        // With short sentences and 40-char chunks, most chunks should end at a period
        assert!(!chunks.is_empty());
        let ending_at_boundary = chunks
            .iter()
            .filter(|c| {
                let t = c.trim();
                t.ends_with('.') || t.ends_with('!') || t.ends_with('?')
            })
            .count();
        // At least half the chunks should end at a sentence boundary
        assert!(
            ending_at_boundary * 2 >= chunks.len(),
            "Expected most chunks to end at sentence boundary, got {}/{}: {:?}",
            ending_at_boundary,
            chunks.len(),
            chunks
        );
    }
}
