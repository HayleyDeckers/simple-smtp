# Issue Implementation Workflow

We use github issues to track work that needs to be done.
This workflow contains a repeatable process for picking up and implementing GitHub issues.

## 1. Find & Plan

- Browse open issues with `gh issue list` or via browser
- Pick a self-contained issue that fits your time/skill
- Create a plan document outlining approach, key files, and implementation details
- consider browsing crates.io for relevant crates where appropriate. If you find multiple crates which could be appropriate, always ask which one to use.

## 2. Create Feature Branch

- Switch to the main branch
- git pull to make sure we're up to date
- make a new feature branch: `git checkout -b <issue-number>-<short-description>`


## 3. Implement

- Make changes following repo code style, cite and link to RFCs whenever making claims about protocol behaviour
- Add tests for new functionality
- Update example configs/docs as needed

## 4. Local Validation

```bash
cargo fmt --all -- --check                                # Rust formatting
cargo clippy --all-targets --all-features -- -D warnings  # lints
cargo test --workspace                                    # tests
taplo format --check                                      # TOML formatting
```

## 5. Commit & Push

```bash
git add <files>               # NEVER use `git add .`
git commit -m "🐛/⭐/📎 description" # no need to add a scope, should be clear from file + comment + content
git push -u origin <branch-name>
```


### Commit Emoji Conventions

| Emoji | Use                |
| ----- | ------------------ |
| ⭐     | Features           |
| 🐛🔨    | Bug fixes          |
| 📎     | Clippy fixes       |
| 📖     | Documentation      |
| 🧹     | Clean-up           |
| 🔒⬆️    | upgrade Cargo.lock |

### Splitting Commits

Keep commits atomic and logically grouped. Here's how to think about it:

**By category:**
- Bug fixes, features, docs, and refactors should generally be separate commits
- CI/tooling changes get their own commit

**Bug fixes:**
- Group related fixes into one commit if they're fixing the same underlying issue
- Split into separate commits if they're fixing *unrelated* bugs
- Example: "Auth wasn't storing mechanisms" + "supports() wasn't checking mechanism list" = same commit (both part of AUTH being broken)
- Example: "Fix panic on empty buffer" vs "Fix off-by-one in iterator" = separate commits (unrelated bugs)

**Tests & docs:**
- Tests go *with* the feature/bugfix they're testing (same commit)
- If adding tests for *existing* code with no associated change, make it a separate commit
- Same rule for docs: with the change if documenting new stuff, separate if documenting existing code

**When in doubt:**
- Ask: "Could someone revert just this commit and have things still make sense?"
- If reverting would leave the codebase in a broken state, the commits are too granular
- If reverting removes unrelated changes, the commit is too chunky

## 6. Create PR

```bash
gh pr create --title "⭐ scope: description" --body "Closes #<issue>" --base <main-branch>
```

Or use GitHub web UI if branch names are funky.

## 7. Monitor CI

- Check PR checks page for CI status
- Fix any failures (fmt, clippy, tests, taplo, etc.)
- Amend commit and force push fixes:

```bash
git add <fixed-files>
git commit --amend --no-edit
git push --force-with-lease
```

## 8. Review & Merge

- Wait for CI green ✅
- If CI fails, iterate and ammend the original MR





