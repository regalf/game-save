# Contributing to Game Save

Thank you for helping. This file is short on purpose: it holds the rules that
keep the project healthy and the time of everybody who reads it.

There are two rules that matter more than everything else here. They are the
first two sections, on purpose.

---

## 1. One issue, one problem

An issue describes **one** problem. One bug, one missing feature, one
question, one inconsistency.

If you find two things, open two issues and link them to each other. The
reasons are practical:

- a fix that touches two unrelated things is harder to review and easier to get
  wrong;
- an issue that mixes two problems hides the one that nobody is working on;
- a change that fixes two problems at once cannot be reverted without losing
  the other fix.

The same goes for the discussion inside an issue: if it turns into two
problems, split it.

**Before opening an issue**, please search: this project is small and the
answer is often already there.

---

## 2. One pull request, one fix

A pull request fixes **one** issue, and says which one.

- Reference the issue in the title or in the description: `Fixes #42`.
- A pull request that touches more than one thing gets asked to split. This is
  not bureaucracy: a reviewer who has to understand four changes at once
  usually understands none of them.
- Unrelated changes that are obviously right (a typo, a formatting fix) go in
  their own pull request, with the same rules.
- A pull request without an issue is fine for a small fix, but say so in the
  description: "no issue, small fix".

**One pull request, one concern.** If the reviewer asks for something that
turns it into a second thing, that is the moment to split.

---

## 3. If you use an AI agent

If an AI agent helped you write the code, say so, **at the end of the
description**, as the very last line, in this exact form:

```markdown
<!-- ai: agent=claude-code model=claude-sonnet-4.5 task=implement-delta-upload -->
```

- `agent` — the name of the tool: `claude-code`, `cursor`, `codex`, `copilot`,
  `gemini-cli`, `aider`, `windsurf`, or whatever you used.
- `model` — the model that wrote the code, as the tool names it.
- `task` — a few words about what the agent did.

If more than one agent was involved, one line for each, in the same format.

If no agent was involved, **do not write anything**. There is no need for an
"ai: none" line, and a missing line is the answer.

Why we ask: to know how much of the project is written with the help of these
tools, and to be able to ask better questions about how the code was made. The
line is a comment: it does not clutter the conversation.

**The rules of this file apply the same way.** Code written by an agent is
your code: you have read it, you understand it, and you answer for it. A
pull request that dumps generated code nobody has read will not be merged,
whatever wrote it.

---

## 4. Before you open a pull request

```bash
# the server
cd server
go vet ./...
go test ./...

# the client
cd ../desktop
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The end to end test runs the real client against a real server, both started
on the spot:

```powershell
./scripts/e2e.ps1
```

If the continuous integration is red, the pull request is not ready.

**Please keep the pull request focused.** A pull request that reformats
everything is impossible to review. If a file needs reformatting, do it in its
own pull request.

---

## 5. Commits

- One commit does one thing.
- The subject line says what changed, in the imperative: "Fix the path with
  spaces", not "fixed the path".
- No "wip", no "fix", no "address review comments" in the history: squash
  before asking for a review, it is kinder to the reader.

---

## 6. The licence

Game Save is under the **GPLv3**. By contributing you agree that your
contribution is published under the same licence.

If you copy code from somewhere else, say so in the pull request, and make
sure the licence allows it. The font in `desktop/assets/fonts` is under the
SIL Open Font License, which is compatible with the GPL.

---

## 7. Being decent to each other

The short version: argue about the code, never about the person.

- Say what the problem is, not who caused it.
- A review is about the change, and the author is not the change.
- "I do not like this" is fine. "This is stupid" is not, and it makes the
  project worse for everybody.
- Somebody who knows less than you is not stupid: they are learning, and you
  were learning too.
- When you disagree, say why, and be willing to be convinced.

The longer version is in [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), which is
also linked by GitHub on every issue and pull request.

---

## 8. Security

If the thing you found is a vulnerability, **do not open an issue**: read
[SECURITY.md](SECURITY.md) first. It says how to report it privately, what the
program is built to withstand, and why it must not be exposed to the internet.

---

## 9. Questions

If something is unclear, open an issue. If the answer helps more than one
person, put it in the documentation afterwards.

There is no private channel for the project. Public is fine.
