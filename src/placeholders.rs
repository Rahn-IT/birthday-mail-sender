pub struct PlaceholderSpan {
    pub name: Vec<u8>,
    pub start: usize,
    pub end: usize,
}

pub fn locate_placeholders(content: &[u8], name: &[u8]) -> Vec<(usize, usize)> {
    if name.is_empty() || content.len() < 4 {
        return Vec::new();
    }

    let mut matches: Vec<(usize, usize)> = Vec::new();
    let mut offset = 0;
    while let Some(found) = locate_any_placeholder(content, offset) {
        if found.name.as_slice() == name {
            matches.push((found.start, found.end));
        }
        offset = found.end;
    }

    matches
}

pub fn locate_any_placeholder(content: &[u8], from: usize) -> Option<PlaceholderSpan> {
    if content.len() < 4 || from >= content.len() {
        return None;
    }

    let mut i = from;
    while i + 1 < content.len() {
        if content[i] != b'{' || content[i + 1] != b'{' {
            i += 1;
            continue;
        }

        let mut j = i + 2;
        while j < content.len() && content[j].is_ascii_whitespace() {
            j += 1;
        }

        let name_start = j;
        while j < content.len()
            && !content[j].is_ascii_whitespace()
            && content[j] != b'}'
            && content[j] != b'{'
        {
            j += 1;
        }
        let name_end = j;

        if name_start == name_end {
            i += 1;
            continue;
        }

        while j < content.len() && content[j].is_ascii_whitespace() {
            j += 1;
        }

        if j + 1 < content.len() && content[j] == b'}' && content[j + 1] == b'}' {
            return Some(PlaceholderSpan {
                name: content[name_start..name_end].to_vec(),
                start: i,
                end: j + 2,
            });
        }

        i += 1;
    }

    None
}
