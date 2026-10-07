//! Startup refusal of compose limit settings. Its own test binary with one
//! test, because it sets process environment variables.
use aien_runtime::spine::{compose_budgets_from_env, compose_doc_max_tokens_from_env};
use std::time::Duration;

#[test]
fn environment_limits_are_read_and_bad_ones_refused() {
    let all = [
        "AIEN_COMPOSE_EDIT_BUDGET_MS",
        "AIEN_COMPOSE_DOC_BUDGET_MS",
        "AIEN_COMPOSE_DOC_MAX_TOKENS",
        "AIEN_COMPOSE_BUDGET_MS",
    ];
    for k in all {
        std::env::remove_var(k);
    }
    let b = compose_budgets_from_env().expect("defaults");
    assert_eq!(b.edit, Duration::from_secs(29));
    assert_eq!(b.doc, Duration::from_secs(120));
    assert_eq!(compose_doc_max_tokens_from_env(), Ok(1024));

    std::env::set_var("AIEN_COMPOSE_DOC_BUDGET_MS", "90000");
    std::env::set_var("AIEN_COMPOSE_DOC_MAX_TOKENS", "512");
    assert_eq!(
        compose_budgets_from_env().expect("set").doc,
        Duration::from_secs(90)
    );
    assert_eq!(compose_doc_max_tokens_from_env(), Ok(512));

    std::env::set_var("AIEN_COMPOSE_DOC_BUDGET_MS", "5");
    assert!(compose_budgets_from_env()
        .expect_err("out of range")
        .contains("AIEN_COMPOSE_DOC_BUDGET_MS"));
    std::env::set_var("AIEN_COMPOSE_DOC_BUDGET_MS", "90000");
    std::env::set_var("AIEN_COMPOSE_DOC_MAX_TOKENS", "0");
    assert!(compose_doc_max_tokens_from_env().is_err());

    // the retired single-budget name is refused, not ignored
    std::env::set_var("AIEN_COMPOSE_BUDGET_MS", "29000");
    assert!(compose_budgets_from_env()
        .expect_err("retired")
        .contains("retired"));
    for k in all {
        std::env::remove_var(k);
    }
}
