//! Ports `Deslop/Casing.hs` — casing convention checks used by rulebook
//! module-name constraints.
//!
//! TODO(port): exact classification set from the original (camel, snake,
//! kebab, pascal, screaming snake, …).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Casing {
    Camel,
    Pascal,
    Snake,
    ScreamingSnake,
    Kebab,
    Unknown,
}

pub fn classify(s: &str) -> Casing {
    if s.is_empty() {
        return Casing::Unknown;
    }
    let has_upper = s.chars().any(char::is_uppercase);
    let has_lower = s.chars().any(char::is_lowercase);
    let has_underscore = s.contains('_');
    let has_hyphen = s.contains('-');
    match (has_underscore, has_hyphen, has_upper, has_lower) {
        (true, _, true, false) => Casing::ScreamingSnake,
        (true, _, false, _) => Casing::Snake,
        (_, true, _, _) => Casing::Kebab,
        (false, false, true, true) => {
            if s.chars().next().map_or(false, char::is_uppercase) {
                Casing::Pascal
            } else {
                Casing::Camel
            }
        }
        _ => Casing::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_casings() {
        assert_eq!(classify("myModule"), Casing::Camel);
        assert_eq!(classify("MyModule"), Casing::Pascal);
        assert_eq!(classify("my_module"), Casing::Snake);
        assert_eq!(classify("MY_MODULE"), Casing::ScreamingSnake);
        assert_eq!(classify("my-module"), Casing::Kebab);
    }
}
