//! Mirror suite of the original's GlobPlusSpec.hs + GlobPlusSemanticsSpec.hs,
//! ported case-for-case so the Rust matcher is pinned to the same behavior.

use crate::casing::Casing;
use crate::glob_plus::compiler::{compile_clause_pattern, compile_exclude_pattern, compile_target_pattern, GlobPlusError};
use crate::glob_plus::{
    match_clause, match_target, module_from_glob, render_clause_pattern, segments_of,
    ClauseVar, MatchEnv, Polarity, Segments,
};
use std::collections::BTreeMap;

fn target(pat: &str) -> crate::glob_plus::CompiledTargetPattern {
    compile_target_pattern(pat).unwrap_or_else(|e| panic!("did not compile {pat:?}: {e:?}"))
}

/// envFor: match a target pattern against a path, panicking if no match.
fn env_for(pat: &str, path: &str) -> MatchEnv {
    match_target(&target(pat), &segments_of(path))
        .unwrap_or_else(|| panic!("{pat} did not match {path}"))
}

fn sparse_env() -> MatchEnv {
    MatchEnv { target_dir: "@/features/x".into(), variables: BTreeMap::new() }
}

fn clause_in(bound: &[String], pat: &str) -> crate::glob_plus::CompiledClausePattern {
    clause_as(Polarity::Narrow, bound, pat)
}

fn clause_as(
    polarity: Polarity,
    bound: &[String],
    pat: &str,
) -> crate::glob_plus::CompiledClausePattern {
    compile_clause_pattern(polarity, bound, pat)
        .unwrap_or_else(|e| panic!("clause did not compile {pat:?}: {e:?}"))
}

fn kebab_of(name: &str, env: &MatchEnv) -> Option<String> {
    env.variables.get(name).map(|b| b.spelling.kebab.clone())
}

fn casing_of(casing: Casing, name: &str, env: &MatchEnv) -> Option<String> {
    env.variables.get(name).map(|b| match casing {
        Casing::Pascal => b.spelling.pascal.clone(),
        Casing::Camel => b.spelling.camel.clone(),
        Casing::Kebab => b.spelling.kebab.clone(),
        Casing::Constant => b.spelling.constant.clone(),
    })
}

// ---------------------------------------------------------------------------
// matchTarget
// ---------------------------------------------------------------------------

#[test]
fn matches_exact_literal_paths_and_derives_target_dir() {
    let t = target("src/app/page");
    assert_eq!(match_target(&t, &segments_of("src/app/page")).unwrap().target_dir, "src/app");
    assert!(match_target(&t, &segments_of("src/app/other")).is_none());
}

#[test]
fn wildcards_and_directory_derivation() {
    let t = target("@/features/**/components/*");
    assert_eq!(
        match_target(&t, &segments_of("@/features/users/auth/components/Button"))
            .unwrap()
            .target_dir,
        "@/features/users/auth/components"
    );
    let star = target("@/features/*/page");
    assert!(match_target(&star, &segments_of("@/features/auth/login/page")).is_none());
    assert!(match_target(&star, &segments_of("@/features/home/page")).is_some());
}

#[test]
fn extracts_file_name_and_enriches_all_casings() {
    let env = env_for("@/features/{{FileName}}View", "@/features/UserSettingsView");
    assert_eq!(env.target_dir, "@/features");
    assert_eq!(casing_of(Casing::Pascal, "file-name", &env).as_deref(), Some("UserSettings"));
    assert_eq!(casing_of(Casing::Camel, "file-name", &env).as_deref(), Some("userSettings"));
    assert_eq!(casing_of(Casing::Kebab, "file-name", &env).as_deref(), Some("user-settings"));
    assert_eq!(casing_of(Casing::Constant, "file-name", &env).as_deref(), Some("USER_SETTINGS"));
    // Casing of the token constrains what it captures.
    assert!(match_target(&target("@/features/{{FileName}}View"), &segments_of("@/features/userSettingsView")).is_none());
}

#[test]
fn extracts_camel_kebab_constant_tokens() {
    let env = env_for("@/features/{{fileName}}Controller", "@/features/userProfileController");
    assert_eq!(casing_of(Casing::Camel, "file-name", &env).as_deref(), Some("userProfile"));
    assert_eq!(casing_of(Casing::Pascal, "file-name", &env).as_deref(), Some("UserProfile"));

    let env = env_for("@/features/{{file-name}}-repository", "@/features/user-settings-repository");
    assert_eq!(casing_of(Casing::Kebab, "file-name", &env).as_deref(), Some("user-settings"));

    let env = env_for("src/constants/{{FILE_NAME}}", "src/constants/MAX_RETRY_COUNT");
    assert_eq!(env.target_dir, "src/constants");
    assert_eq!(casing_of(Casing::Constant, "file-name", &env).as_deref(), Some("MAX_RETRY_COUNT"));
    assert_eq!(casing_of(Casing::Pascal, "file-name", &env).as_deref(), Some("MaxRetryCount"));
    assert!(match_target(&target("src/constants/{{FILE_NAME}}"), &segments_of("src/constants/maxRetryCount")).is_none());
    assert!(match_target(&target("src/constants/{{FILE_NAME}}"), &segments_of("src/constants/max-retry-count")).is_none());
}

#[test]
fn literal_prefix_and_suffix_around_variables() {
    let env = env_for("@/features/**/use{{FileName}}ViewModel", "@/features/auth/useUserAuthViewModel");
    assert_eq!(env.target_dir, "@/features/auth");
    assert_eq!(casing_of(Casing::Pascal, "file-name", &env).as_deref(), Some("UserAuth"));
    assert!(match_target(&target("@/features/**/use{{FileName}}ViewModel"), &segments_of("@/features/auth/getUserAuthViewModel")).is_none());

    let env = env_for("@/features/**/use{{FileName}}ViewModel.spec", "@/features/auth/useUserAuthViewModel.spec");
    assert_eq!(casing_of(Casing::Pascal, "file-name", &env).as_deref(), Some("UserAuth"));
}

#[test]
fn single_word_and_compound_enrichment() {
    let env = env_for("@/features/{{FileName}}View", "@/features/HomeView");
    assert_eq!(casing_of(Casing::Kebab, "file-name", &env).as_deref(), Some("home"));

    let env = env_for("@/features/**/{{FileName}}Container", "@/features/admin/UserProfileSettingsContainer");
    assert_eq!(casing_of(Casing::Pascal, "file-name", &env).as_deref(), Some("UserProfileSettings"));
    assert_eq!(casing_of(Casing::Kebab, "file-name", &env).as_deref(), Some("user-profile-settings"));

    let env = env_for("@/features/**/{{FileName}}Container", "@/features/auth/oauth/google/GoogleAuthContainer");
    assert_eq!(env.target_dir, "@/features/auth/oauth/google");

    // Root-level file: empty TARGET_DIR.
    let env = env_for("{{FileName}}", "HomeView");
    assert_eq!(env.target_dir, "");
}

#[test]
fn globstar_semantics() {
    // ** stands for zero.
    assert!(match_target(&target("@/lib/**"), &segments_of("@/lib")).is_some());
    assert!(match_target(&target("@/lib/**"), &segments_of("@/libs")).is_none());
    assert!(match_target(&target("@/lib/**/*"), &segments_of("@/lib")).is_none());
    assert!(match_target(&target("@/lib/**/*"), &segments_of("@/lib/jwt")).is_some());
    assert!(match_target(&target("**/*"), &segments_of("jwt")).is_some());
    // Bare prefix with trailing globstar.
    assert!(match_target(&target("@/a/**/b/**/*"), &segments_of("@/a/b/c")).is_some());
    assert!(match_target(&target("@/a/**/b/**/*"), &segments_of("@/a/x/y/b/z/w/c")).is_some());
    // * is exactly one segment when alone.
    let star = target("@/lib/*");
    assert!(match_target(&star, &segments_of("@/lib/jwt")).is_some());
    assert!(match_target(&star, &segments_of("@/lib/auth/jwt")).is_none());
    assert!(match_target(&star, &segments_of("@/lib")).is_none());
    // ** never glues to text.
    let seg = target("**/components");
    assert!(match_target(&seg, &segments_of("xcomponents")).is_none());
    assert!(match_target(&seg, &segments_of("components/child")).is_none());
    // Dots are ordinary characters.
    let dots = target("src/utils.lib/{{FileName}}");
    assert!(match_target(&dots, &segments_of("src/utils.lib/HomeView")).is_some());
    assert!(match_target(&dots, &segments_of("src/utilsXlib/HomeView")).is_none());
}

#[test]
fn acronym_capture_preserved_and_read_coarsely() {
    let env = env_for("@/services/{{FileName}}Client", "@/services/HTTPClient");
    assert_eq!(casing_of(Casing::Pascal, "file-name", &env).as_deref(), Some("HTTP"));
    assert_eq!(casing_of(Casing::Kebab, "file-name", &env).as_deref(), Some("http"));

    let env = env_for("@/services/{{FileName}}Service", "@/services/OAuth2Service");
    assert_eq!(casing_of(Casing::Pascal, "file-name", &env).as_deref(), Some("OAuth2"));
    assert_eq!(casing_of(Casing::Kebab, "file-name", &env).as_deref(), Some("o-auth2"));

    let widget = |p: &str| {
        env_for("@/widgets/{{FileName}}Widget", p)
    };
    assert_eq!(kebab_of("file-name", &widget("@/widgets/DBConnectionWidget")).as_deref(), Some("db-connection"));
    assert_eq!(kebab_of("file-name", &widget("@/widgets/AWSS3Widget")).as_deref(), Some("awss3"));
    assert_eq!(kebab_of("file-name", &widget("@/widgets/ABTestWidget")).as_deref(), Some("ab-test"));
}

// ---------------------------------------------------------------------------
// Semantics matrix (GlobPlusSemanticsSpec.hs)
// ---------------------------------------------------------------------------

#[test]
fn a_variable_pinned_by_literal_on_one_side() {
    let pat = target("@/components/**/{{provider-name}}/{{FileName}}View");
    for path in [
        "@/components/stripe-connect/CheckoutView",
        "@/components/a/stripe-connect/CheckoutView",
        "@/components/a/b/stripe-connect/CheckoutView",
    ] {
        let env = match_target(&pat, &segments_of(path)).unwrap();
        assert_eq!(kebab_of("provider-name", &env).as_deref(), Some("stripe-connect"));
        assert_eq!(kebab_of("file-name", &env).as_deref(), Some("checkout"));
    }
    // Repeated variable straddling a globstar, either direction.
    let env = env_for(
        "@/components/{{provider-name}}/**/{{ProviderName}}View",
        "@/components/stripe-connect/a/b/StripeConnectView",
    );
    assert_eq!(kebab_of("provider-name", &env).as_deref(), Some("stripe-connect"));
    let env = env_for(
        "@/**/{{provider-name}}/{{ProviderName}}View",
        "@/a/b/stripe-connect/StripeConnectView",
    );
    assert_eq!(kebab_of("provider-name", &env).as_deref(), Some("stripe-connect"));
    // Same segment at every depth: pattern decides, not the path.
    let pat = target("@/{{provider-name}}/**/{{FileName}}View");
    for path in [
        "@/stripe-connect/CheckoutView",
        "@/stripe-connect/payment/CheckoutView",
        "@/stripe-connect/payment/gateway/CheckoutView",
    ] {
        let env = match_target(&pat, &segments_of(path)).unwrap();
        assert_eq!(kebab_of("provider-name", &env).as_deref(), Some("stripe-connect"));
    }
}

#[test]
fn unanchored_variables_are_compile_errors() {
    let scope = vec!["provider-name".to_string()];
    let err = compile_target_pattern("@/**/{{provider-name}}/**/{{FileName}}View").unwrap_err();
    assert_eq!(err, GlobPlusError::UnanchoredVariable("provider-name".to_string()));
    let err = compile_target_pattern("@/**/{{provider-name}}/**").unwrap_err();
    assert_eq!(err, GlobPlusError::UnanchoredVariable("provider-name".to_string()));
    let err = compile_target_pattern("@/{{provider-name}}/**/{{service-type}}/**/{{FileName}}View").unwrap_err();
    assert_eq!(err, GlobPlusError::UnanchoredVariable("service-type".to_string()));
    // Anchored from one side only: fine. Clauses are exempt.
    assert!(compile_target_pattern("@/{{provider-name}}/**/{{FileName}}View").is_ok());
    assert!(compile_target_pattern("@/**/{{provider-name}}/{{FileName}}View").is_ok());
    assert!(compile_clause_pattern(Polarity::Narrow, &scope, "@/**/{{provider-name}}/**").is_ok());
}

#[test]
fn d_separator_both_variables_could_consume() {
    // Greedy-left by default.
    let env = env_for("@/c/{{provider-name}}-{{service-type}}", "@/c/stripe-connect-payment");
    assert_eq!(kebab_of("provider-name", &env).as_deref(), Some("stripe-connect"));
    assert_eq!(kebab_of("service-type", &env).as_deref(), Some("payment"));
    // The only split there is.
    let env = env_for("@/c/{{provider-name}}-{{service-type}}", "@/c/stripe-payment");
    assert_eq!(kebab_of("provider-name", &env).as_deref(), Some("stripe"));
    // Agreement participates in choosing the split.
    let env = env_for(
        "@/c/{{provider-name}}/{{provider-name}}-{{service-type}}",
        "@/c/stripe/stripe-connect-payment",
    );
    assert_eq!(kebab_of("provider-name", &env).as_deref(), Some("stripe"));
    assert_eq!(kebab_of("service-type", &env).as_deref(), Some("connect-payment"));
    // No split agrees -> no match; exact agreement matches.
    assert!(match_target(&target("@/{{provider-name}}/{{provider-name}}-service"), &segments_of("@/paypal/stripe-service")).is_none());
    assert!(match_target(&target("@/{{provider-name}}/{{provider-name}}-service"), &segments_of("@/stripe-connect/stripe-connect-service")).is_some());
}

#[test]
fn e_two_variables_need_a_literal_between_them() {
    let e = || compile_target_pattern("@/x/{{FileName}}{{ServiceType}}").unwrap_err();
    assert_eq!(e(), GlobPlusError::NoBoundaryBetween("FileName".into(), "ServiceType".into()));
    let e = compile_target_pattern("@/x/{{FileName}}*{{ServiceType}}").unwrap_err();
    assert_eq!(e, GlobPlusError::NoBoundaryBetween("FileName".into(), "ServiceType".into()));
    assert!(compile_target_pattern("@/x/{{provider-name}}-{{service-type}}").is_ok());
}

#[test]
fn f_globstar_zero_everywhere_and_star_within_segment() {
    // F. zero-width globstar catches folder module itself.
    let env = env_for("@/components/{{provider-name}}/**", "@/components/stripe-connect");
    assert_eq!(kebab_of("provider-name", &env).as_deref(), Some("stripe-connect"));
    let env = env_for("@/components/{{provider-name}}/**", "@/components/stripe-connect/a/b");
    assert_eq!(kebab_of("provider-name", &env).as_deref(), Some("stripe-connect"));
    assert!(match_target(&target("@/**/{{FileName}}View"), &segments_of("@/CheckoutView")).is_some());
    // Idempotence of adjacent globstars.
    for path in ["@/a/b", "@/a/x/b", "@/a/x/y/b"] {
        assert_eq!(
            match_target(&target("@/a/**/**/b"), &segments_of(path)).is_some(),
            match_target(&target("@/a/**/b"), &segments_of(path)).is_some(),
        );
    }
    // * between literals, matching nothing at all, and never crossing /.
    assert!(match_target(&target("@/features/**/use*ViewModel"), &segments_of("@/features/home/useHomeViewModel")).is_some());
    assert!(match_target(&target("@/a/use*ViewModel"), &segments_of("@/a/useViewModel")).is_some());
    assert!(match_target(&target("@/features/*/page"), &segments_of("@/features/auth/login/page")).is_none());
    assert!(match_target(&target("@/features/*/page"), &segments_of("@/features/page")).is_none());
}

// ---------------------------------------------------------------------------
// Clauses
// ---------------------------------------------------------------------------

#[test]
fn clauses_hydrate_variables_and_wildcards() {
    let sample = env_for("@/features/user/{{FileName}}", "@/features/user/UserSettings");
    let rich = env_for("@/features/home/{{FileName}}", "@/features/home/HomeProfile");
    let file_name_scope = ["file-name".to_string()];

    let rule = clause_in(&[], "{{TARGET_DIR}}/data/repository");
    assert!(match_clause(&rule, &sample, &segments_of("@/features/user/data/repository")));

    let rule = clause_in(&file_name_scope, "{{TARGET_DIR}}/data/{{file-name}}-repository");
    assert!(match_clause(&rule, &sample, &segments_of("@/features/user/data/user-settings-repository")));
    assert!(!match_clause(&rule, &sample, &segments_of("@/features/other/data/user-settings-repository")));
    assert!(!match_clause(&rule, &sample, &segments_of("@/features/user/data/UserSettings-repository")));

    let rule = clause_in(&file_name_scope, "{{TARGET_DIR}}/**/*{{FileName}}*");
    assert!(match_clause(&rule, &sample, &segments_of("@/features/user/components/buttons/UserSettingsButton")));
    assert!(!match_clause(&rule, &sample, &segments_of("@/features/user/components/buttons/OtherButton")));

    let rule = clause_in(&file_name_scope, "{{TARGET_DIR}}/{{FILE_NAME}}_config");
    assert!(match_clause(&rule, &rich, &segments_of("@/features/home/HOME_PROFILE_config")));
    assert!(!match_clause(&rule, &rich, &segments_of("@/features/home/home-profile_config")));

    // Sparse env fails closed.
    let rule = clause_in(&file_name_scope, "{{TARGET_DIR}}/{{FileName}}View");
    assert!(!match_clause(&rule, &sparse_env(), &segments_of("@/features/x/AnythingView")));
    assert!(!match_clause(&rule, &sparse_env(), &segments_of("@/features/other/AnythingView")));

    // Regex metacharacters in TARGET_DIR stay literal.
    let dot_env = env_for("src/v1.0/features/{{FileName}}", "src/v1.0/features/Home");
    let rule = clause_in(&file_name_scope, "{{TARGET_DIR}}/{{FileName}}View");
    assert!(match_clause(&rule, &dot_env, &segments_of("src/v1.0/features/HomeView")));
    assert!(!match_clause(&rule, &dot_env, &segments_of("src/v1X0/features/HomeView")));
}

#[test]
fn g_parent_dirs_reach_sideways() {
    let feature_env = env_for("@/client/{{feature-name}}/{{FileName}}View", "@/client/home/HomeView");
    let scope = ["feature-name".to_string()];
    let allows =
        |pat: &str| clause_in(&scope, pat);

    let rule = allows("{{TARGET_DIR}}/../shared/**");
    assert!(match_clause(&rule, &feature_env, &segments_of("@/client/shared/Button")));
    assert!(match_clause(&rule, &feature_env, &segments_of("@/client/shared/forms/Input")));
    assert!(!match_clause(&rule, &feature_env, &segments_of("@/client/home/shared/Button")));
    assert!(!match_clause(&rule, &feature_env, &segments_of("@/client/billing/shared/Button")));

    let rule = allows("{{TARGET_DIR}}/../../shared/**");
    assert!(match_clause(&rule, &feature_env, &segments_of("@/shared/Button")));
    assert!(!match_clause(&rule, &feature_env, &segments_of("@/client/shared/Button")));

    // Clamp at the front.
    let rule = allows("{{TARGET_DIR}}/../../../shared/**");
    assert!(match_clause(&rule, &feature_env, &segments_of("shared/Button")));
    let rule = allows("../shared/**");
    assert!(match_clause(&rule, &feature_env, &segments_of("shared/Button")));

    // Depth-dependent TARGET_DIR.
    let deep_env = env_for("@/client/{{feature-name}}/**/{{FileName}}View", "@/client/home/widgets/CardView");
    let rule = allows("{{TARGET_DIR}}/../shared/**");
    assert!(match_clause(&rule, &deep_env, &segments_of("@/client/home/shared/Icon")));
    assert!(!match_clause(&rule, &deep_env, &segments_of("@/client/shared/Button")));

    // Cancelling literals, variables, and exactly one dir per ..
    let rule = allows("@/client/home/../shared/**");
    assert!(match_clause(&rule, &feature_env, &segments_of("@/client/shared/Button")));
    assert!(!match_clause(&rule, &feature_env, &segments_of("@/client/home/Button")));
    let rule = allows("@/client/{{feature-name}}/../shared/**");
    assert!(match_clause(&rule, &feature_env, &segments_of("@/client/shared/Button")));
    let rule = allows("{{TARGET_DIR}}/..");
    assert!(match_clause(&rule, &feature_env, &segments_of("@/client")));
    assert!(!match_clause(&rule, &feature_env, &segments_of("@")));
    // Dotted non-.. segments are text.
    let rule = allows("{{TARGET_DIR}}/../..shared/x");
    assert!(match_clause(&rule, &feature_env, &segments_of("@/client/..shared/x")));
    // A ** not being cancelled stays put.
    let rule = allows("@/client/**/widgets/../shared/**");
    assert!(match_clause(&rule, &feature_env, &segments_of("@/client/shared/Button")));
    assert!(match_clause(&rule, &feature_env, &segments_of("@/client/home/shared/Button")));
}

#[test]
fn parent_dir_compile_rules() {
    let scope: Vec<String> = ["provider-name", "service-type", "file-name"]
        .iter().map(|s| s.to_string()).collect();
    let e = compile_clause_pattern(Polarity::Narrow, &scope, "@/client/**/../shared").unwrap_err();
    assert_eq!(e, GlobPlusError::ParentDirPastWildcard("**".into()));
    let e = compile_clause_pattern(Polarity::Narrow, &scope, "@/client/*/../shared").unwrap_err();
    assert_eq!(e, GlobPlusError::ParentDirPastWildcard("*".into()));
    let e = compile_clause_pattern(Polarity::Narrow, &scope, "@/client/*View/../shared").unwrap_err();
    assert_eq!(e, GlobPlusError::ParentDirPastWildcard("*View".into()));
    // Chain checked one .. at a time against what each reaches.
    assert!(compile_clause_pattern(Polarity::Narrow, &scope, "@/a*/b/../shared").is_ok());
    let e = compile_clause_pattern(Polarity::Narrow, &scope, "@/a*/b/../../shared").unwrap_err();
    assert_eq!(e, GlobPlusError::ParentDirPastWildcard("a*".into()));
    // Past literal, variable, TARGET_DIR: allowed.
    assert!(compile_clause_pattern(Polarity::Narrow, &scope, "@/client/home/../shared").is_ok());
    assert!(compile_clause_pattern(Polarity::Narrow, &scope, "@/client/{{provider-name}}/../shared").is_ok());
    assert!(compile_clause_pattern(Polarity::Narrow, &scope, "{{TARGET_DIR}}/../shared").is_ok());
    // .. in target/exclude patterns rejected; dotted names are plain text.
    assert_eq!(
        compile_target_pattern("@/client/../shared/**").unwrap_err(),
        GlobPlusError::ParentDirInTargetPattern
    );
    assert_eq!(
        compile_exclude_pattern("@/client/../shared/**").unwrap_err(),
        GlobPlusError::ParentDirInExcludePattern
    );
    assert!(compile_target_pattern("@/client/..shared/x").is_ok());
    assert!(compile_target_pattern("@/client/.../x").is_ok());
    assert!(compile_target_pattern("@/client/a..b/x").is_ok());
    // Glued globstars.
    assert_eq!(
        compile_target_pattern("@/a/**View").unwrap_err(),
        GlobPlusError::GlobStarNotWholeSegment("**View".into())
    );
    assert_eq!(
        compile_target_pattern("@/a/View**").unwrap_err(),
        GlobPlusError::GlobStarNotWholeSegment("View**".into())
    );
}

// ---------------------------------------------------------------------------
// Expansion
// ---------------------------------------------------------------------------

#[test]
fn module_from_glob_expansion() {
    let env = env_for("@/features/auth/{{FileName}}", "@/features/auth/UserAuth");
    let expand = |pat: &str| module_from_glob(&env, &clause_in(&["file-name".to_string()], pat));

    assert_eq!(expand("{{TARGET_DIR}}/use{{FileName}}ViewModel.spec").as_deref(), Some("@/features/auth/useUserAuthViewModel.spec"));
    assert_eq!(expand("{{TARGET_DIR}}/{{file-name}}-repository").as_deref(), Some("@/features/auth/user-auth-repository"));
    assert_eq!(expand("{{TARGET_DIR}}/index").as_deref(), Some("@/features/auth/index"));
    assert_eq!(expand("@/shared/constants").as_deref(), Some("@/shared/constants"));
    assert_eq!(expand("{{TARGET_DIR}}/*.spec"), None);
    assert_eq!(expand("{{TARGET_DIR}}/**/*.spec"), None);
    assert_eq!(module_from_glob(&env, &clause_in(&[], "**")), None);
    // Unbound variable: no concrete module can be named.
    assert_eq!(module_from_glob(&sparse_env(), &clause_in(&["file-name".to_string()], "{{TARGET_DIR}}/{{FileName}}View")), None);

    // Resolving .. after substitution.
    assert_eq!(expand("{{TARGET_DIR}}/../shared/registry").as_deref(), Some("@/features/shared/registry"));
    assert_eq!(expand("{{TARGET_DIR}}/../../shared/registry").as_deref(), Some("@/shared/registry"));
    assert_eq!(expand("@/features/auth/../shared/registry").as_deref(), Some("@/features/shared/registry"));
    assert_eq!(expand("{{TARGET_DIR}}/../../../../shared/registry").as_deref(), Some("shared/registry"));
    assert_eq!(expand("{{TARGET_DIR}}/../shared/*"), None);
}

#[test]
fn expansion_acronym_and_multi_word() {
    let widget_of = |p: &str| env_for("@/widgets/{{FileName}}Widget", p);
    let config_of = |p: &str| {
        module_from_glob(&widget_of(p), &clause_in(&["file-name".to_string()], "@/config/{{file-name}}"))
    };
    assert_eq!(config_of("@/widgets/DBConnectionWidget").as_deref(), Some("@/config/db-connection"));
    assert_eq!(config_of("@/widgets/HTTPClientWidget").as_deref(), Some("@/config/http-client"));
    assert_eq!(config_of("@/widgets/AWSS3Widget").as_deref(), Some("@/config/awss3"));
    let pascal_of = |p: &str| {
        module_from_glob(&widget_of(p), &clause_in(&["file-name".to_string()], "@/config/{{FileName}}"))
    };
    assert_eq!(pascal_of("@/widgets/AWSS3Widget").as_deref(), Some("@/config/AWSS3"));

    // Paired capture keeps the exact spelling per casing slot.
    let paired = target("@/components/{{provider-name}}/{{ProviderName}}View");
    let paired_env =
        match_target(&paired, &segments_of("@/components/aws-s3/AWSS3View")).unwrap();
    let scope = paired.bound_vars.clone();
    assert_eq!(
        module_from_glob(&paired_env, &clause_in(&scope, "@/config/{{provider-name}}")).as_deref(),
        Some("@/config/aws-s3")
    );
    assert_eq!(
        module_from_glob(&paired_env, &clause_in(&scope, "@/config/{{ProviderName}}")).as_deref(),
        Some("@/config/AWSS3")
    );

    // Three-word name through all casings.
    let uc = env_for("@/application/{{UseCaseName}}UseCase", "@/application/ArchiveOrderUseCase");
    let scope2 = ["use-case-name".to_string()];
    assert_eq!(
        module_from_glob(&uc, &clause_in(&scope2, "@/x/{{UseCaseName}}/{{useCaseName}}/{{use-case-name}}/{{USE_CASE_NAME}}")).as_deref(),
        Some("@/x/ArchiveOrder/archiveOrder/archive-order/ARCHIVE_ORDER")
    );
}

#[test]
fn render_clause_pattern_cases() {
    let env = env_for("@/features/auth/{{FileName}}", "@/features/auth/UserAuth");
    let r = |pat: &str| render_clause_pattern(&env, &clause_in(&["file-name".to_string()], pat));
    assert_eq!(r("{{TARGET_DIR}}/{{FileName}}StateEvent"), "@/features/auth/UserAuthStateEvent");
    assert_eq!(r("{{TARGET_DIR}}/use{{FileName}}ViewModel"), "@/features/auth/useUserAuthViewModel");
    assert_eq!(r("@/features/**/*Container"), "@/features/**/*Container");
    assert_eq!(r("{{TARGET_DIR}}/**/*.spec"), "@/features/auth/**/*.spec");
    assert_eq!(r("{{TARGET_DIR}}/../shared/registry"), "@/features/shared/registry");
    assert_eq!(r("{{TARGET_DIR}}/../../shared/registry"), "@/shared/registry");
    assert_eq!(r("{{TARGET_DIR}}/../**/*.spec"), "@/features/**/*.spec");
    // Falls back to as-written when unbound.
    assert_eq!(
        render_clause_pattern(&sparse_env(), &clause_in(&["file-name".to_string()], "{{TARGET_DIR}}/{{FileName}}View")),
        "@/features/x/{{FileName}}View"
    );
}

// ---------------------------------------------------------------------------
// Interpolation (prose)
// ---------------------------------------------------------------------------

#[test]
fn interpolate_prose() {
    use crate::glob_plus::compiler::interpolate;
    let env = env_for("@/features/auth/{{FileName}}", "@/features/auth/UserAuth");
    let i = |s: &str| interpolate(&env, s);

    assert_eq!(i("{{FileName}} {{fileName}} {{file-name}} {{FILE_NAME}}"), "UserAuth userAuth user-auth USER_AUTH");
    assert_eq!(i("Add a spec next to {{TARGET_DIR}}."), "Add a spec next to @/features/auth.");
    assert_eq!(
        i("Import use{{FileName}}ViewModel and drive {{FileName}}View from it."),
        "Import useUserAuthViewModel and drive UserAuthView from it."
    );
    assert_eq!(i("Promote the shared code out of the provider folders."), "Promote the shared code out of the provider folders.");
    assert_eq!(i(""), "");
    assert_eq!(i("Move **/*.spec files under {{TARGET_DIR}}/tests."), "Move **/*.spec files under @/features/auth/tests.");
    assert_eq!(i("Add a {{FileNam}}View."), "Add a {{FileNam}}View.");
    assert_eq!(i("Import {{provider-name}}."), "Import {{provider-name}}.");
    assert_eq!(i("Rename {{File_Name}}."), "Rename {{File_Name}}.");
    assert_eq!(i("Add {{ FileName }}."), "Add {{ FileName }}.");
    assert_eq!(i("See {{a/b}} and {{a*b}}."), "See {{a/b}} and {{a*b}}.");
    assert_eq!(i("Braces are written {{}}."), "Braces are written {{}}.");
    assert_eq!(i("An unfinished {{FileName is just prose."), "An unfinished {{FileName is just prose.");
    assert_eq!(i("{{outer {{FileName}}}"), "{{outer UserAuth}");
    // Sparse env: TARGET_DIR substitutes, unbound variables stay as written.
    assert_eq!(i("Look in {{TARGET_DIR}} for {{FileName}}."), "Look in @/features/auth for UserAuth.");
    assert_eq!(
        interpolate(&sparse_env(), "Look in {{TARGET_DIR}} for {{FileName}}."),
        "Look in @/features/x for {{FileName}}."
    );

    let provider_env = env_for(
        "@/components/{{provider-name}}/{{service-type}}/{{FileName}}View",
        "@/components/stripe-connect/payment/CheckoutView",
    );
    assert_eq!(
        interpolate(
            &provider_env,
            "Import @/services/{{provider-name}}/{{service-type}}-{{file-name}} from {{TARGET_DIR}}."
        ),
        "Import @/services/stripe-connect/payment-checkout from @/components/stripe-connect/payment."
    );
}

// ---------------------------------------------------------------------------
// Polarity
// ---------------------------------------------------------------------------

#[test]
fn polarity_widen_vs_narrow() {
    let t = target("@/widgets/{{file-name}}");
    let scope = t.bound_vars.clone();
    let env_of = |p: &str| env_for("@/widgets/{{file-name}}", p);

    let forbidding = clause_as(Polarity::Widen, &scope, "@/internal/{{FileName}}/**");
    let requiring = clause_as(Polarity::Narrow, &scope, "@/internal/{{FileName}}/**");
    let env = env_of("@/widgets/db-connection");
    assert!(match_clause(&forbidding, &env, &segments_of("@/internal/DbConnection/x")));
    assert!(match_clause(&forbidding, &env, &segments_of("@/internal/DBConnection/x")));
    assert!(match_clause(&requiring, &env, &segments_of("@/internal/DbConnection/x")));
    assert!(!match_clause(&requiring, &env, &segments_of("@/internal/DBConnection/x")));

    // Ambiguous capture widens over every reading.
    let ab_env = env_for("@/widgets/{{FileName}}", "@/widgets/ABTest");
    let forbids_kebab = clause_as(Polarity::Widen, &["file-name".to_string()], "@/internal/{{file-name}}/**");
    assert!(match_clause(&forbids_kebab, &ab_env, &segments_of("@/internal/ab-test/x")));
    assert!(match_clause(&forbids_kebab, &ab_env, &segments_of("@/internal/a-b-test/x")));

    // Single-spelling casings do not widen.
    let kebab_clause =
        |p: Polarity| clause_as(p, &scope, "@/internal/{{file-name}}/**");
    assert!(match_clause(&kebab_clause(Polarity::Widen), &env, &segments_of("@/internal/db-connection/x")));
    assert!(!match_clause(&kebab_clause(Polarity::Widen), &env, &segments_of("@/internal/dbConnection/x")));

    // Several variables widen/narrow independently.
    let provider_target = target("@/components/{{provider-name}}/{{service-type}}/{{FileName}}View");
    let provider_scope = provider_target.bound_vars.clone();
    let env_p = env_for(
        "@/components/{{provider-name}}/{{service-type}}/{{FileName}}View",
        "@/components/stripe-connect/payment/CheckoutView",
    );
    let clause_of = |p: Polarity| clause_as(p, &provider_scope, "@/x/{{ProviderName}}/{{ServiceType}}/{{file-name}}");
    for p in [Polarity::Narrow, Polarity::Widen] {
        let c = clause_of(p);
        assert!(match_clause(&c, &env_p, &segments_of("@/x/StripeConnect/Payment/checkout")));
        assert!(!match_clause(&c, &env_p, &segments_of("@/x/Paypal/Payment/checkout")));
        assert!(!match_clause(&c, &env_p, &segments_of("@/x/StripeConnect/Payout/checkout")));
        assert!(!match_clause(&c, &env_p, &segments_of("@/x/StripeConnect/Payment/refund")));
    }
    let w = clause_of(Polarity::Widen);
    assert!(match_clause(&w, &env_p, &segments_of("@/x/STRIPEConnect/Payment/checkout")));
    assert!(match_clause(&w, &env_p, &segments_of("@/x/StripeConnect/PAYMENT/checkout")));
    assert!(match_clause(&w, &env_p, &segments_of("@/x/STRIPECONNECT/PAYMENT/checkout")));
    let n = clause_of(Polarity::Narrow);
    assert!(!match_clause(&n, &env_p, &segments_of("@/x/STRIPEConnect/Payment/checkout")));
    assert!(!match_clause(&n, &env_p, &segments_of("@/x/StripeConnect/PAYMENT/checkout")));

    // Literal capture stays exact under both polarities.
    let pascal_t = target("@/widgets/{{FileName}}");
    let pascal_scope = pascal_t.bound_vars.clone();
    let env_db = env_for("@/widgets/{{FileName}}", "@/widgets/DBConnection");
    let c = |p: Polarity| clause_as(p, &pascal_scope, "@/internal/{{FileName}}/**");
    for p in [Polarity::Narrow, Polarity::Widen] {
        assert!(match_clause(&c(p), &env_db, &segments_of("@/internal/DBConnection/x")));
    }

    // Adjacent variables in a clause are fine under both polarities.
    let adj = |p: Polarity| clause_as(p, &provider_scope, "@/x/{{ServiceType}}{{FileName}}");
    for p in [Polarity::Narrow, Polarity::Widen] {
        assert!(match_clause(&adj(p), &env_p, &segments_of("@/x/PaymentCheckout")));
    }
    assert!(match_clause(&adj(Polarity::Widen), &env_p, &segments_of("@/x/PAYMENTCheckout")));
    assert!(!match_clause(&adj(Polarity::Narrow), &env_p, &segments_of("@/x/PAYMENTCheckout")));
}

// ---------------------------------------------------------------------------
// Compile errors: casing rules
// ---------------------------------------------------------------------------

#[test]
fn casing_detection_errors() {
    fn err_of(r: Result<crate::glob_plus::CompiledTargetPattern, GlobPlusError>) -> Option<GlobPlusError> {
        r.err()
    }
    assert_eq!(
        err_of(compile_target_pattern("@/x/{{provider}}")),
        Some(GlobPlusError::AmbiguousCasing { raw: "provider".into(), casings: vec![Casing::Camel, Casing::Kebab] })
    );
    assert_eq!(
        err_of(compile_target_pattern("@/x/{{PROVIDER}}")),
        Some(GlobPlusError::AmbiguousCasing { raw: "PROVIDER".into(), casings: vec![Casing::Pascal, Casing::Constant] })
    );
    assert!(err_of(compile_target_pattern("@/x/{{Provider}}")).is_none());
    assert_eq!(
        err_of(compile_target_pattern("@/x/{{Provider-Name}}")),
        Some(GlobPlusError::UnrecognisedCasing("Provider-Name".into()))
    );
    assert_eq!(
        err_of(compile_target_pattern("@/x/{{provider_name}}")),
        Some(GlobPlusError::UnrecognisedCasing("provider_name".into()))
    );
    assert_eq!(
        err_of(compile_target_pattern("@/x/{{HTTPClient}}")),
        Some(GlobPlusError::ConsecutiveCapitals("HTTPClient".into()))
    );
    assert_eq!(
        err_of(compile_target_pattern("@/x/{{httpAPIClient}}")),
        Some(GlobPlusError::ConsecutiveCapitals("httpAPIClient".into()))
    );
    assert!(err_of(compile_target_pattern("@/x/{{HttpClient}}")).is_none());
    assert!(err_of(compile_target_pattern("@/x/{{http-client}}")).is_none());
    assert_eq!(
        err_of(compile_target_pattern("@/x/{{UseCASEName}}")),
        Some(GlobPlusError::ConsecutiveCapitals("UseCASEName".into()))
    );
}

#[test]
fn reserved_and_unbound_variables() {
    let e = compile_clause_pattern(Polarity::Narrow, &[], "{{target-dir}}/x").unwrap_err();
    assert_eq!(e, GlobPlusError::ReservedTargetDir("target-dir".into()));
    let e = compile_clause_pattern(Polarity::Narrow, &[], "{{targetDir}}/x").unwrap_err();
    assert_eq!(e, GlobPlusError::ReservedTargetDir("targetDir".into()));
    let e = compile_clause_pattern(Polarity::Narrow, &[], "{{TargetDir}}/x").unwrap_err();
    assert_eq!(e, GlobPlusError::ReservedTargetDir("TargetDir".into()));
    assert!(compile_clause_pattern(Polarity::Narrow, &[], "{{TARGET_DIR}}/x").is_ok());
    let e = compile_target_pattern("{{TARGET_DIR}}/x").unwrap_err();
    assert_eq!(e, GlobPlusError::TargetDirInTargetPattern("TARGET_DIR".into()));
    let e = compile_target_pattern("{{target-dir}}/x").unwrap_err();
    assert_eq!(e, GlobPlusError::TargetDirInTargetPattern("target-dir".into()));

    let e = compile_exclude_pattern("@/x/{{FileName}}").unwrap_err();
    assert_eq!(e, GlobPlusError::VariableInExcludePattern("FileName".into()));
    let e = compile_exclude_pattern("@/x/{{TARGET_DIR}}").unwrap_err();
    assert_eq!(e, GlobPlusError::VariableInExcludePattern("TARGET_DIR".into()));
    assert!(compile_exclude_pattern("@/x/**/*.spec").is_ok());

    let e = compile_clause_pattern(Polarity::Narrow, &["file-name".to_string()], "{{TARGET_DIR}}/{{provider-name}}").unwrap_err();
    assert_eq!(
        e,
        GlobPlusError::UnboundVariable { name: "provider-name".into(), bound: vec!["file-name".into()] }
    );

    // Adjacent variables allowed in clauses (substituted, not captured).
    assert!(compile_clause_pattern(
        Polarity::Narrow,
        &["file-name".to_string(), "service-type".to_string()],
        "@/x/{{FileName}}{{ServiceType}}"
    ).is_ok());

    // Malformed.
    assert!(matches!(
        compile_target_pattern("@/x/{{unclosed"),
        Err(GlobPlusError::MalformedPattern { .. })
    ));
}

// ---------------------------------------------------------------------------
// Rendered messages
// ---------------------------------------------------------------------------

#[test]
fn rendered_error_messages() {
    let msg = crate::glob_plus::compiler::render_error(
        &compile_target_pattern("@/x/{{provider}}").unwrap_err(),
    );
    assert!(msg.contains("camelCase and kebab-case"), "{msg}");
    assert!(msg.contains("{{providerName}}"), "{msg}");
    assert!(msg.contains("{{provider-name}}"), "{msg}");

    let scope = vec!["provider-name".to_string(), "file-name".to_string()];
    let msg = crate::glob_plus::compiler::render_error(
        &compile_clause_pattern(Polarity::Narrow, &scope, "{{TARGET_DIR}}/{{provider-nam}}").unwrap_err(),
    );
    assert!(msg.contains("file-name, provider-name"), "{msg}");
    assert!(msg.contains("Did you mean {{provider-name}}?"), "{msg}");

    let msg = crate::glob_plus::compiler::render_error(
        &compile_clause_pattern(Polarity::Narrow, &[], "{{target-dir}}/x").unwrap_err(),
    );
    assert!(msg.contains("{{TARGET_DIR}}"), "{msg}");

    let msg = crate::glob_plus::compiler::render_error(
        &compile_target_pattern("@/client/../shared/**").unwrap_err(),
    );
    assert!(msg.contains("\"..\" cannot be used in a target pattern."), "{msg}");
    assert!(msg.contains("Write the path you mean"), "{msg}");
    assert!(msg.contains("{{TARGET_DIR}}/../shared/**"), "{msg}");

    let msg = crate::glob_plus::compiler::render_error(
        &compile_exclude_pattern("@/client/../shared/**").unwrap_err(),
    );
    assert!(msg.contains("\"..\" cannot be used in an exclude pattern."), "{msg}");

    let msg = crate::glob_plus::compiler::render_error(
        &compile_clause_pattern(Polarity::Narrow, &[], "@/client/**/../shared").unwrap_err(),
    );
    assert!(msg.contains("\"..\" cannot go back past \"**\"."), "{msg}");
    assert!(msg.contains("zero or many segments"), "{msg}");

    let msg = crate::glob_plus::compiler::render_error(
        &compile_clause_pattern(Polarity::Narrow, &[], "@/client/*View/../shared").unwrap_err(),
    );
    assert!(msg.contains("\"..\" cannot go back past \"*View\"."), "{msg}");
    assert!(msg.contains("does not say which directory it is"), "{msg}");

    // A .. that goes back past a determined segment compiles.
    assert!(compile_clause_pattern(Polarity::Narrow, &[], "@/client/home/../shared").is_ok());
    assert!(compile_clause_pattern(Polarity::Narrow, &[], "{{TARGET_DIR}}/../shared").is_ok());
    assert!(compile_clause_pattern(Polarity::Narrow, &[], "../../shared").is_ok());
}
