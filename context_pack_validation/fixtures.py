from __future__ import annotations

from pathlib import Path


def build_fixture(name: str, root: Path) -> Path:
    repo = root / name
    if name == "best_auth":
        _best_auth(repo)
    elif name == "review_payments":
        _review_payments(repo)
    elif name == "normal_config":
        _normal_config(repo)
    elif name == "worst_vague":
        _worst_vague(repo)
    elif name == "tiny_overhead":
        _tiny_overhead(repo)
    else:
        raise ValueError(f"unknown context-pack fixture `{name}`")
    return repo


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content.lstrip(), encoding="utf-8")


def _best_auth(repo: Path) -> None:
    _write(
        repo / "src/auth.rs",
        """
        pub struct AuthConfig {
            pub service_account: bool,
        }

        pub fn validate_service_account(config: &AuthConfig) {
            if config.service_account {
                panic!("auth panic for service account");
            }
        }
        """,
    )
    _write(
        repo / "tests/auth_test.rs",
        """
        #[test]
        fn service_account_auth_does_not_panic() {
            assert!(true);
        }
        """,
    )
    _write(repo / "src/main.rs", "fn main() {}\n")


def _review_payments(repo: Path) -> None:
    _write(
        repo / "src/payments/processor.rs",
        """
        pub fn process_payment(amount: u64) -> bool {
            amount > 0
        }
        """,
    )
    _write(
        repo / "tests/payments_test.rs",
        """
        #[test]
        fn process_payment_accepts_positive_amounts() {
            assert!(true);
        }
        """,
    )
    _write(repo / "README.md", "# Payments fixture\n")


def _normal_config(repo: Path) -> None:
    _write(
        repo / "src/server/config.rs",
        """
        pub struct RetryConfig {
            pub retry_backoff_ms: u64,
        }
        """,
    )
    _write(
        repo / "src/server/client.rs",
        """
        use super::config::RetryConfig;

        pub fn retry_backoff(config: RetryConfig) -> u64 {
            config.retry_backoff_ms
        }
        """,
    )
    _write(repo / "tests/retry_config_test.rs", "#[test]\nfn retry_config_loads() {}\n")


def _worst_vague(repo: Path) -> None:
    for idx in range(12):
        _write(
            repo / f"docs/update_{idx}.md",
            f"# Update notes {idx}\n\nThis update document mentions update many times.\n",
        )
    _write(
        repo / "src/core/real_target.rs",
        """
        pub fn apply_customer_state_transition() -> bool {
            true
        }
        """,
    )
    _write(repo / "src/lib.rs", "pub mod core;\n")


def _tiny_overhead(repo: Path) -> None:
    _write(repo / "src/lib.rs", "pub fn small_bug() { panic!(\"small bug\"); }\n")
