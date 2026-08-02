# Development Rules

These are the rules every implementation in this codebase must follow. They are
not suggestions. When a rule and a shortcut conflict, the rule wins.

## 1. Elegance and simplicity above all

The goal of every implementation is a solution that is **beautiful, elegant,
clever, and simple** — in that order of intent, but never at the expense of
simplicity.

- **Minimize complexity.** The best code is the code you didn't have to write.
  Before adding a layer, an abstraction, or a dependency, prove it earns its
  place. Prefer the solution with the fewest moving parts that fully solves the
  problem.
- **Solve the real problem, not the imagined one.** Do not build for
  requirements that don't exist yet. Speculative generality is complexity in
  disguise.
- **Cleverness serves clarity, never ego.** A clever solution is one that makes
  a hard problem look easy. If a reader needs a comment to understand _what_ the
  code does (as opposed to _why_), it is not yet elegant — rewrite it.
- **One responsibility per unit.** Each function, module, and type should do one
  thing and have one reason to change. Small, composable pieces beat large,
  monolithic ones.
- **Delete before you add.** Reducing code is progress. When a change lets you
  remove something, remove it.

## 2. Readability and maintainability

Code is read far more often than it is written. Optimize for the reader.

- **Names carry meaning.** Choose names that describe intent precisely. A good
  name removes the need for a comment.
- **Keep it flat and linear.** Prefer early returns over deep nesting. Prefer
  straight-line logic over hidden control flow.
- **Comments explain _why_, not _what_.** The code shows what happens; comments
  justify non-obvious decisions, trade-offs, and constraints.
- **Consistency over personal preference.** Follow the conventions already
  present in the codebase — style, structure, error handling, and naming — even
  where you would have chosen differently.
- **Leave the code better than you found it.** Small, incidental cleanups are
  welcome, but keep them separate from feature changes so history stays
  readable.

## 3. Clean code principles

Apply the established principles of clean code throughout:

- **DRY** — remove duplication of knowledge, not just of text.
- **KISS** — keep it simple; the simplest thing that works is usually right.
- **YAGNI** — you aren't going to need it; don't build it until you do.
- **Single Responsibility** — one reason to change per unit.
- **Least surprise** — code should behave the way its name and shape suggest.
- **Fail loudly and early** — surface errors at their source with clear
  context; never swallow them silently.
- **Pure where possible** — isolate side effects; prefer functions whose output
  depends only on their input.

## 4. Respect the specification

When a problem is **well-known and well-documented**, the specification is the
source of truth — not intuition, not a half-remembered example, not a shortcut
that "seems to work."

- **Follow the RFCs (and equivalent standards) to the letter.** For anything
  governed by a published standard — HTTP, TLS, OAuth 2.0 / OIDC, JWT/JOSE,
  TOTP/HOTP, X.509/PKI, DNS, and so on — implement the specification exactly as
  written, including its MUST, MUST NOT, SHOULD, and MAY requirements.
- **Cite the spec.** When implementing a spec-governed behavior, reference the
  relevant document and section in a comment (e.g. `// RFC 6238 §4`) so the
  next reader can verify correctness against the source.
- **Do not reinvent solved problems.** If a well-established, correct approach
  exists, use it. Rolling your own is justified only when no standard fits, and
  that decision must be documented.
- **Never weaken a spec for convenience.** Especially in security, correctness,
  and interoperability, deviating from the standard is a defect — even if it
  passes the happy-path tests.
- **When the spec is ambiguous, prefer the interpretation that maximizes
  interoperability** and document the choice.

## The test for every change

Before considering an implementation done, it should pass all of these:

1. Is this the simplest solution that fully solves the problem?
2. Could a new reader understand it without asking me?
3. Have I removed everything that isn't needed?
4. If a standard governs this, does it match the standard exactly?
5. Would I be glad to maintain this in a year?

If the answer to any of these is _no_, the work is not finished.
