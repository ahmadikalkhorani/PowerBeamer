# Contributing to PowerBeamer & Release Guide

## Development Setup

```bash
# Clone the repository
git clone https://github.com/ahmadikalkhorani/PowerBeamer.git
cd PowerBeamer

# Run test suite (all 46 tests)
cargo test --test presenter_tests

# Run locally with demo deck
cargo run
```

---

## 📦 Publishing Releases (Maintainers Only)

PowerBeamer uses automated multi-platform GitHub Actions to build and publish releases.

### Method 1: Using the Interactive Script
```bash
./scripts/publish_to_github.sh
```

### Method 2: Manual Git Tagging
1. Ensure your local `main` branch is up to date:
   ```bash
   git checkout main
   git pull origin main
   ```
2. Create and push a semver tag:
   ```bash
   git tag v0.1.1 -m "PowerBeamer Release v0.1.1"
   git push origin v0.1.1
   ```
3. GitHub Actions will automatically:
   - Build macOS Apple Silicon (`arm64`), macOS Intel (`x86_64`), and Linux binaries.
   - Generate `SHA256SUMS.txt`.
   - Publish the artifacts to [GitHub Releases](https://github.com/ahmadikalkhorani/PowerBeamer/releases).
