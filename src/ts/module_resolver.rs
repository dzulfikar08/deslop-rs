//! Ports `TypeScript/ModuleResolver.hs`.
//!
//! A `ModuleId` is the logical TS module identity — an alias-mapped path like
//! `@/lib/util`, a relative specifier like `./LoginView`, or a plain absolute
//! path. Files get ids by reverse-resolving their absolute paths through the
//! tsconfig; imports get ids by resolving then reverse-resolving, which is
//! what lets graph edges meet even when spelled differently at each end.

use std::path::{Path, PathBuf};

use crate::ts::config::{Pattern, TsConfig};

/// Ports `TypeScript.ModuleResolver.ModuleId`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(pub String);

impl ModuleId {
    pub fn text(&self) -> &str {
        &self.0
    }
}

pub fn module_id_unsafe(t: impl Into<String>) -> ModuleId {
    ModuleId(t.into())
}

/// Ports `Match`: whether a pattern matched exactly or through its wildcard,
/// carrying the capture when it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    ExactMatch,
    WildcardMatch(String),
}

/// Ports `match`: exact equality, or prefix/capture/suffix matching where the
/// wildcard must capture at least zero characters — never a partial fit.
pub fn match_pattern(p: &Pattern, t: &str) -> Option<Match> {
    match p {
        Pattern::Exact(pre) => (pre == t).then_some(Match::ExactMatch),
        Pattern::Wildcard { pre, suff } => {
            let rest = t.strip_prefix(pre)?;
            let capture = rest.strip_suffix(suff)?;
            // Length guard mirrors T.length check; strip_prefix/suffix cannot
            // over-consume, so this only rejects when pre+suff exceed t.
            if t.chars().count() >= pre.chars().count() + suff.chars().count() {
                Some(Match::WildcardMatch(capture.to_string()))
            } else {
                None
            }
        }
    }
}

/// Ports `isRelativeImport`: `.`, `..`, anything starting `./`, `../`, `/`.
pub fn is_relative_import(m: &ModuleId) -> bool {
    let t = m.text();
    t == "." || t == ".." || t.starts_with("./") || t.starts_with("../") || t.starts_with('/')
}

/// Ports `dropTypeScriptExtension`. Declaration files lose both extensions
/// (`user.d.ts` → `user`); ordinary script extensions lose one.
pub fn drop_type_script_extension(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let decl = [".d.ts", ".d.mts", ".d.cts"].iter().any(|e| text.ends_with(e));
    let script = [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|e| text.ends_with(e));
    if decl {
        path.with_extension("").with_extension("")
    } else if script {
        path.with_extension("")
    } else {
        path.to_path_buf()
    }
}

// ---------------------------------------------------------------------------
// reverseResolve: absolute path -> alias ModuleId
// ---------------------------------------------------------------------------

/// Ports `reverseResolve`. The file's extension-dropped path loses whatever
/// segments it shares with baseUrl; the remaining baseUrl depth becomes that
/// many `..` segments; the joined relative id goes through the path mappings
/// in their sorted order. A mapping hit that still reads relative (the alias
/// itself was relative) resolves to nothing.
pub fn reverse_resolve(cfg: &TsConfig, abs_file_path: &Path) -> Option<ModuleId> {
    let no_ext = drop_type_script_extension(abs_file_path);
    let target_segs = split_directories(&no_ext);
    let base_segs = split_directories(&cfg.base_url);
    let (t_remainder, b_remainder) = drop_common_segments(&target_segs, &base_segs);

    let mut parts: Vec<String> = vec!["..".to_string(); b_remainder.len()];
    parts.extend(t_remainder.iter().cloned());
    let module_rel_to_cfg = parts.join("/");

    apply_path_mapping(&cfg.paths, &module_rel_to_cfg).and_then(|alias| {
        let module_id = ModuleId(alias);
        (!is_relative_import(&module_id)).then_some(module_id)
    })
}

fn split_directories(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn drop_common_segments<'a>(xs: &'a [String], ys: &'a [String]) -> (&'a [String], &'a [String]) {
    match (xs.split_first(), ys.split_first()) {
        (Some((x, xs_tail)), Some((y, ys_tail))) if x == y => drop_common_segments(xs_tail, ys_tail),
        _ => (xs, ys),
    }
}

/// Ports `applyPathMapping`: first mapping whose values produce a match wins;
/// a wildcard capture substitutes into a wildcard key's pre/suff, while an
/// exact key can never consume a wildcard capture (fall-through to later
/// mappings instead).
fn apply_path_mapping(paths: &[crate::ts::config::PathMapping], module_rel_to_cfg: &str) -> Option<String> {
    paths.iter().find_map(|x| {
        let value_match = x.values.iter().find_map(|v| match_pattern(v, module_rel_to_cfg))?;
        match (value_match, &x.key) {
            (Match::ExactMatch, Pattern::Exact(t)) => Some(t.clone()),
            (Match::ExactMatch, Pattern::Wildcard { pre, suff }) => {
                Some(format!("{pre}{suff}"))
            }
            (Match::WildcardMatch(_), Pattern::Exact(_)) => None,
            (Match::WildcardMatch(capture), Pattern::Wildcard { pre, suff }) => {
                Some(format!("{pre}{capture}{suff}"))
            }
        }
    })
}

// ---------------------------------------------------------------------------
// resolve: import target -> absolute path
// ---------------------------------------------------------------------------

const TS_EXTENSIONS: &[&str] = &[".ts", ".tsx", "/index.ts", "/index.tsx"];

/// Ports `resolve`: relative targets resolve against the importing file's
/// directory; everything else goes through the tsconfig mappings in order.
pub fn resolve(cfg: &TsConfig, importing_file: &Path, target: &ModuleId) -> Option<PathBuf> {
    if is_relative_import(target) {
        let importer_dir = importing_file.parent().unwrap_or(Path::new("."));
        let target_path = clean_relative(importer_dir.join(target.text()));
        // Ports `maybe (fsMkAbsolute targetPath) pure`: an import whose
        // extension probes all miss still resolves — to its own path.
        try_extensions(&target_path).or(Some(target_path))
    } else {
        cfg.paths.iter().find_map(|p| {
            let key_match = match_pattern(&p.key, target.text())?;
            p.values.iter().find_map(|v| {
                let maybe_rel = match (&key_match, v) {
                    // Invalid shape: exact key matched but wildcard value.
                    (Match::ExactMatch, Pattern::Wildcard { .. }) => None,
                    (Match::ExactMatch, Pattern::Exact(t)) => Some(t.clone()),
                    (Match::WildcardMatch(_), Pattern::Exact(t)) => Some(t.clone()),
                    (Match::WildcardMatch(capture), Pattern::Wildcard { pre, suff }) => {
                        Some(format!("{pre}{capture}{suff}"))
                    }
                };
                let clean = maybe_rel.map(|r| r.trim_end_matches('/').to_string());
                let rel = clean?;
                let file_path = cfg.base_url.join(rel);
                try_extensions(&file_path)
            })
        })
    }
}

/// Collapses `.` and `..` in a joined relative path lexically.
fn clean_relative(p: PathBuf) -> PathBuf {
    let mut out: PathBuf = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn try_extensions(fp: &Path) -> Option<PathBuf> {
    TS_EXTENSIONS.iter().find_map(|ext| {
        let candidate = PathBuf::from(format!("{}{ext}", fp.display()));
        candidate.is_file().then_some(candidate)
    })
}

// ---------------------------------------------------------------------------
// programModuleId
// ---------------------------------------------------------------------------

/// Ports `TypeScript.AST.parseAst`'s `programModuleId`: a file's own id must
/// come from the same alias mapping its import edges use, or the two never
/// meet in the graph. `reverseResolve` works from the path directly, which
/// both OSes split correctly; the raw extension-dropped path remains the
/// fallback for unmapped files.
pub fn program_module_id(cfg: &TsConfig, path: &Path) -> ModuleId {
    reverse_resolve(cfg, path).unwrap_or_else(|| {
        module_id_unsafe(
            drop_type_script_extension(path).to_string_lossy().replace('\\', "/"),
        )
    })
}

// ---------------------------------------------------------------------------
// reverseResolveImport
// ---------------------------------------------------------------------------

/// Ports `reverseResolveImport`: an import's canonical id is what its resolved
/// file reverse-resolves to — unless that would be `<target>/index`, in which
/// case the original spelling already named the directory implicitly and is
/// kept. Unresolvable imports keep their original target.
pub fn reverse_resolve_import(
    cfg: &TsConfig,
    importing_file: &Path,
    target: &ModuleId,
) -> ModuleId {
    let maybe_abs_path = resolve(cfg, importing_file, target);
    match maybe_abs_path {
        Some(abs_path) => match reverse_resolve(cfg, &abs_path) {
            None => target.clone(),
            Some(resolved) => {
                if resolved.text() == format!("{}/index", target.text()) {
                    target.clone()
                } else {
                    resolved
                }
            }
        },
        None => target.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ts::config::PathMapping;

    fn cfg(base: &str, mappings: &[(&str, &[&str])]) -> TsConfig {
        TsConfig {
            base_url: PathBuf::from(base),
            paths: mappings
                .iter()
                .map(|(k, vs)| {
                    PathMapping::parse(
                        k,
                        &vs.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                    )
                    .unwrap()
                })
                .collect(),
        }
    }

    /// ModuleResolverSpec "match TSConfig pattern" tables.
    #[test]
    fn match_pattern_cases() {
        let exact = Pattern::Exact("react".into());
        assert_eq!(match_pattern(&exact, "react"), Some(Match::ExactMatch));
        assert_eq!(match_pattern(&exact, "react-dom"), None);
        assert_eq!(match_pattern(&exact, "React"), None);
        assert_eq!(match_pattern(&exact, ""), None);

        let suffix = Pattern::Wildcard { pre: "src/".into(), suff: "".into() };
        assert_eq!(match_pattern(&suffix, "src/"), Some(Match::WildcardMatch("".into())));
        assert_eq!(
            match_pattern(&suffix, "src/components/ui/button.tsx"),
            Some(Match::WildcardMatch("components/ui/button.tsx".into()))
        );
        assert_eq!(match_pattern(&suffix, "test/util.ts"), None);
        assert_eq!(match_pattern(&suffix, "sr/page.tsx"), None);

        let infix = Pattern::Wildcard { pre: "@types/".into(), suff: "-dto".into() };
        assert_eq!(match_pattern(&infix, "@types/-dto"), Some(Match::WildcardMatch("".into())));
        assert_eq!(match_pattern(&infix, "@types/a-dto"), Some(Match::WildcardMatch("a".into())));
        assert_eq!(
            match_pattern(&infix, "@types/user-reg/old/something-dto"),
            Some(Match::WildcardMatch("user-reg/old/something".into()))
        );
        assert_eq!(match_pattern(&infix, "@types/a-dtoS"), None);
        assert_eq!(match_pattern(&infix, "@lib/a-dto"), None);

        let prefix = Pattern::Wildcard { pre: "".into(), suff: "-spec.ts".into() };
        assert_eq!(match_pattern(&prefix, "-spec.ts"), Some(Match::WildcardMatch("".into())));
        assert_eq!(
            match_pattern(&prefix, "src/components/button-spec.ts"),
            Some(Match::WildcardMatch("src/components/button".into()))
        );
        assert_eq!(match_pattern(&prefix, "auth-spec.tsx"), None);
        assert_eq!(match_pattern(&prefix, "auth.ts"), None);
    }

    #[test]
    fn is_relative_import_cases() {
        for t in [".", "..", "./a", "../a", "/abs"] {
            assert!(is_relative_import(&module_id_unsafe(t)), "{t}");
        }
        for t in ["@/a", "react", "a/b"] {
            assert!(!is_relative_import(&module_id_unsafe(t)), "{t}");
        }
    }

    /// ModuleResolverSpec dropTypeScriptExtension behaviour via golden cases
    /// from other specs (.d.ts double drop; .controller.ts single drop).
    #[test]
    fn drops_extensions_like_haskell() {
        assert_eq!(
            drop_type_script_extension(Path::new("/r/src/types/user.d.ts")),
            PathBuf::from("/r/src/types/user")
        );
        assert_eq!(
            drop_type_script_extension(Path::new("/r/src/api/user.controller.ts")),
            PathBuf::from("/r/src/api/user.controller")
        );
        assert_eq!(
            drop_type_script_extension(Path::new("/r/x.mts")),
            PathBuf::from("/r/x")
        );
        assert_eq!(
            drop_type_script_extension(Path::new("/r/src/a.css")),
            PathBuf::from("/r/src/a.css")
        );
    }

    fn rev(cfg: &TsConfig, p: &str) -> Option<String> {
        reverse_resolve(cfg, Path::new(p)).map(|m| m.0)
    }

    /// ModuleResolverSpec "reverseResolve" table.
    #[test]
    fn reverse_resolve_cases() {
        // No mappings: nothing resolves even inside baseUrl.
        let c = cfg("/home/repo", &[]);
        assert_eq!(rev(&c, "/home/repo/src/lib/util.tsx"), None);
        // Outside baseUrl with no mapping.
        assert_eq!(rev(&c, "/home/shared/utils.ts"), None);
        assert_eq!(rev(&c, "/usr/local/lib/node_modules/react/index.js"), None);

        // Suffix wildcard mapping.
        let c = cfg("/home/repo", &[("@/*", &["src/*"])]);
        assert_eq!(rev(&c, "/home/repo/src/lib/util.tsx"), Some("@/lib/util".into()));
        // Unmapped directory inside baseUrl.
        assert_eq!(rev(&c, "/home/repo/test/util.ts"), None);
        // CSS module keeps its full extension.
        assert_eq!(
            rev(&c, "/home/repo/src/components/Button.module.css"),
            Some("@/components/Button.module.css".into())
        );
        // Index file keeps "index" (the /index special case belongs to
        // reverseResolveImport, not here).
        assert_eq!(rev(&c, "/home/repo/src/lib/utils/index.ts"), Some("@/lib/utils/index".into()));

        // Exact key mapping.
        let c = cfg("/home/repo", &[("jquery", &["node_modules/jquery/dist/jquery"])]);
        assert_eq!(rev(&c, "/home/repo/node_modules/jquery/dist/jquery.js"), Some("jquery".into()));

        // Infix wildcard.
        let c = cfg("/home/repo", &[("@dto/*-dto", &["src/types/*-dto"])]);
        assert_eq!(
            rev(&c, "/home/repo/src/types/user/account-dto.ts"),
            Some("@dto/user/account-dto".into())
        );
        assert_eq!(rev(&c, "/home/repo/src/types/user/account.ts"), None);

        // Prefix wildcard on the value side.
        let c = cfg("/home/repo", &[("@tests/*-spec", &["src/tests/*-spec"])]);
        assert_eq!(rev(&c, "/home/repo/src/tests/auth-spec.ts"), Some("@tests/auth-spec".into()));

        // Multi-value fallback matches the second value.
        let c = cfg("/home/repo", &[("@utils/*", &["src/utils/*", "shared/utils/*"])]);
        assert_eq!(rev(&c, "/home/repo/shared/utils/math.ts"), Some("@utils/math".into()));
        assert_eq!(rev(&c, "/home/repo/src/lib/core.ts"), None);

        // First matched mapping wins (exact listed before wildcard).
        let c = cfg(
            "/home/repo",
            &[("@utils/math", &["src/utils/math"]), ("@utils/*", &["src/utils/*"])],
        );
        assert_eq!(rev(&c, "/home/repo/src/utils/math.ts"), Some("@utils/math".into()));

        // Invalid exact-key/wildcard-value pair falls through to next mapping.
        // The spec builds this illegal shape directly (parse would reject it).
        let c = TsConfig {
            base_url: PathBuf::from("/home/repo"),
            paths: vec![
                PathMapping {
                    key: Pattern::Exact("invalid-exact".into()),
                    values: vec![Pattern::Wildcard { pre: "src/libs/".into(), suff: "".into() }],
                },
                PathMapping::parse("@libs/*", &["src/libs/*".to_string()]).unwrap(),
            ],
        };
        assert_eq!(rev(&c, "/home/repo/src/libs/logger.ts"), Some("@libs/logger".into()));

        // Wildcard keys mapped to exact values produce pre<>capture<>suff of
        // the key: "@core/" <> "" <> "" — the odd-but-faithful shape.
        let c = cfg("/home/repo", &[("@core/*", &["src/core"])]);
        assert_eq!(rev(&c, "/home/repo/src/core.ts"), Some("@core/".into()));

        // Vite-style custom symbol.
        let c = cfg("/home/repo", &[("$utils/*", &["src/utils/*"])]);
        assert_eq!(rev(&c, "/home/repo/src/utils/formatter.ts"), Some("$utils/formatter".into()));
    }
}
