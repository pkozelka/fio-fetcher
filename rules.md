# Development Rules

- One logical fix per commit
- Small, reviewable diffs
- No secrets or API keys in commits
- All examples in examples/ directory, not in main code
- Use `log` crate for logging, not `tracing`
- Commit author: derived from active model
- Never commit directly to main — use PRs, squash-merge
- Feature-gate Google Drive code behind `gdrive` feature