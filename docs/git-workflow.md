# Git Workflow Guide

This guide provides step-by-step instructions for the daily Git operations in the Turnix project.

## 1. Feature Development

Always start by ensuring your local `development` branch is up to date.

```bash
git checkout development
git pull origin development
```

Create your feature branch:

```bash
git checkout -b feature/your-feature-name
```

### Commit Messages
Use the [Conventional Commits](https://www.conventionalcommits.org/) standard:
- `feat: add keyboard driver support`
- `fix: resolve page fault in kernel heap`
- `docs: update build instructions`

## 2. Preparing for Merge

Before opening a PR, rebase your changes onto the latest integration branch.

```bash
git fetch origin
git rebase origin/development
```

If conflicts occur:
1. Resolve them in your editor.
2. `git add <resolved-files>`
3. `git rebase --continue`

## 3. Pull Request Process

1. Push your branch: `git push origin feature/your-feature-name`
2. Open a PR on GitHub targeting the `development` branch.
3. Ensure all CI checks pass.
4. Request a review from at least one core maintainer.

## 4. Hotfixes

Hotfixes are for critical bugs in `main` (production).

```bash
git checkout main
git pull origin main
git checkout -b hotfix/v1.x.x-critical-fix
# ... implement fix ...
git push origin hotfix/v1.x.x-critical-fix
```

After merging the hotfix to `main`, ensure it is also merged back into `development`.

## 5. Branch Protection Rules

The following rules are enforced on the remote repository:

- **`main` and `development`**:
  - Require a pull request before merging.
  - Require status checks to pass (Build & Smoke Test).
  - Require at least 1 approval from a maintainer.
  - No force pushes.
