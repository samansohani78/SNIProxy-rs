# Contributing to SNIProxy-rs

Thank you for your interest in contributing to SNIProxy-rs! This document provides guidelines and information about contributing to this project.

## Development Process

1. Fork the repository
2. Create a feature branch
3. Write code and tests
4. Ensure all tests pass
5. Submit a pull request

## Code Style

We follow standard Rust style guidelines:
- Use `rustfmt` for code formatting
- Follow `clippy` recommendations
- Write descriptive commit messages
- Include documentation for public APIs

## Testing

Before submitting a PR:
```bash
# Run all tests
cargo test

# Run clippy
cargo clippy -- -D warnings

# Check formatting
cargo fmt -- --check
```

## Pull Request Process

1. Update documentation for new features
2. Add tests for new functionality
3. Update the README.md if needed
4. Ensure CI passes on your PR

## Commit Messages

Format:
```
category: short description

Detailed description of changes and reasoning.
```

Categories:
- feat: New features
- fix: Bug fixes
- perf: Performance improvements
- docs: Documentation changes
- test: Test updates
- refactor: Code refactoring
- chore: Maintenance tasks

## Code of Conduct

- Be respectful and inclusive
- Focus on constructive feedback
- Help others learn and grow
- Maintain professional discourse

## Getting Help

- Open an issue for bugs
- Use discussions for questions
- Tag issues appropriately

## License

By contributing, you agree that your contributions will be licensed under the project's MIT License.
