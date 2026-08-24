pub mod cycle_detection;
pub mod relative_imports;

use std::collections::HashMap;

/// The module → imports adjacency map shared by the lint rules.
pub type CodeGraph = HashMap<String, Vec<String>>;
