//! Ports `Utils.hs`.

pub fn pluralise(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singular_and_plural() {
        assert_eq!(pluralise(1, "problem"), "1 problem");
        assert_eq!(pluralise(3, "module"), "3 modules");
        assert_eq!(pluralise(0, "rule"), "0 rules");
    }
}
