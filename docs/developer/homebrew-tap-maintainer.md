# Homebrew Tap Maintainer Guide

This guide covers how to maintain the Homebrew tap for `path`.

## Tap Mapping

- GitHub repository: `compcy/homebrew-tap`
- Homebrew tap name: `compcy/tap`

## Initial Setup

1. Create the repository on GitHub (`compcy/homebrew-tap`).
2. Create the local tap scaffold:

```sh
brew tap-new compcy/homebrew-tap
```

3. Copy the formula from the project repository into the tap:

```sh
cp Formula/path.rb "$(brew --repository compcy/homebrew-tap)/Formula/path.rb"
```

4. Commit and push from the tap repository:

```sh
cd "$(brew --repository compcy/homebrew-tap)"
git add Formula/path.rb
git commit -m "Add path formula"
git remote add origin https://github.com/compcy/homebrew-tap.git
git push -u origin main
```

## Testing the Published Tap

```sh
brew untap mmyers/path-local 2>/dev/null || true
brew tap compcy/tap
brew reinstall compcy/tap/path
brew test compcy/tap/path
```

## Release Update Workflow

When a new `path` version is released:

1. Update `url` and `sha256` in `Formula/path.rb`.
2. Commit and push the formula change in the tap repository.
3. Verify install and test:

```sh
brew update
brew reinstall compcy/tap/path
brew test compcy/tap/path
```

## Expected Installed Paths

Because the formula is keg-only, `path` is not linked into `/opt/homebrew/bin`.

- Binary: `$(brew --prefix)/opt/path/bin/path`
- Wrapper: `$(brew --prefix)/opt/path/share/path/path-wrapper.sh`
