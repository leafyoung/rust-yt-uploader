# Pre-commit Hook

This project uses a comprehensive pre-commit hook to ensure code quality and security before commits are allowed.

## What it checks

The pre-commit hook performs the following checks:

### 🔒 Security Checks

- **Sensitive Files**: Ensures `client_secret.json`, `youtube-oauth2.json`, `.env*` files are properly excluded in `.gitignore`
- **No Sensitive Commits**: Prevents accidental commits of sensitive files
- **Dependency Vulnerabilities**: Runs `cargo audit` if available to check for known security issues

### 🛠️ Code Quality Checks

- **Formatting**: Ensures code is properly formatted with `cargo fmt --check`
- **Linting**: Runs `cargo clippy` with warnings as errors
- **Testing**: Runs unit tests to ensure code functionality

## Installation

The pre-commit hook is automatically installed when you clone the repository. If you need to enable it manually:

```bash
chmod +x .git/hooks/pre-commit
```

## Bypassing the hook (not recommended)

In rare cases where you need to bypass the hook:

```bash
git commit --no-verify -m "Your commit message"
```

**⚠️ WARNING**: Only bypass the hook if you absolutely must, and understand the security implications.

## Troubleshooting

### Hook fails with "cargo-audit not installed"

Install cargo-audit:

```bash
cargo install cargo-audit
```

### Hook fails with formatting errors

Fix formatting:

```bash
cargo fmt
```

### Hook fails with clippy errors

Fix linting issues:

```bash
cargo clippy
```

### Sensitive files detected

Ensure these patterns are in `.gitignore`:

```
client_secret.json
youtube-oauth2.json
.env
.env.local
*.env
```

## Security Benefits

- **Prevents credential leaks**: Stops accidental commits of OAuth tokens and API keys
- **Dependency monitoring**: Alerts to known security vulnerabilities in dependencies
- **Code quality**: Ensures consistent formatting and catches potential bugs
- **Test validation**: Prevents commits that break existing functionality
