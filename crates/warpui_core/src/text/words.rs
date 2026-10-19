/// The default word-boundary characters.
pub const DEFAULT_WORD_BOUNDARY_CHARS: [char; 33] = [
    '`', '~', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '-', '=', '+', '[', '{', ']', '}',
    '\\', '|', ';', ':', '\'', '"', ',', '.', '<', '>', '/', '?', '«', '»',
];

/// Default subword-boundary characters: basically just underscores for now (snake_case)
pub const SUBWORD_BOUNDARY_CHARS: [char; 1] = ['_'];

/// Default logic for determining if a character is a word separator. Word separators are
/// whitespace or a specific set of punctuation characters.
pub fn is_default_word_boundary(c: char) -> bool {
    c.is_whitespace() || DEFAULT_WORD_BOUNDARY_CHARS.contains(&c)
}

/// Logic for determining if a character is a subword separator.
/// Subword separators include all the default word separators
/// (whitespace or a specific set of punctuation characters)
/// and subword-specific separators (underscores, for snake_case).
pub fn is_subword_boundary_char(c: char) -> bool {
    is_default_word_boundary(c) || SUBWORD_BOUNDARY_CHARS.contains(&c)
}
