---
alwaysApply: true
scene: git_message
---

Write your rules here to customize the style of AI-generated commit messages.
1. Project framework versions & dependencies
2. Testing framework details
3. Prohibited APIs
4. Require Pull Request Reviews : Set to at least 1 approval for main and development .
5. Require Status Checks : Enable build and qemu-smoke-test from ci.yml as mandatory.
6. Restrict Force Pushes : Disable for core branches to protect project history.
7. Squash and Merge : Set as the default merge method for feature branches to maintain a clean integration history.